//! Ethernet II

use core::fmt;
use core::ops::{Range, RangeFrom};

use byteorder::{ByteOrder, NetworkEndian as NE};
use cast::{usize, u16};
use as_slice::{AsSlice, AsMutSlice};


use {arp, mac, ipv4};
use {Invalid, Resize};

/* Frame format */
const DESTINATION: Range<usize> = 0..6;
const SOURCE: Range<usize> = 6..12;
const TYPE: Range<usize> = 12..14;
const PAYLOAD: RangeFrom<usize> = 14..;

/// Size of the MAC header
pub const HEADER_SIZE: u16 = TYPE.end as u16;

/// Layer 2 Ethernet frame
///
/// # Structure
///
/// - MAC destination. 6 bytes
/// - MAC source. 6 bytes
/// - Ethertype. 2 bytes
/// - Payload. 46-1500 bytes (\*)
/// - Frame check sequence. 4 bytes (\*)
///
/// (\*) This frame representation does NOT include the frame check sequence nor (zero) pads the
/// payload to the minimum size of 46 bytes.
#[derive(Clone, Copy)]
pub struct Frame<BUFFER>
where
    BUFFER: AsSlice<Element=u8>,
{
    buffer: BUFFER,
}

impl<B> Frame<B>
where
    B: AsSlice<Element=u8>,
{
    /* Constructors */
    /// Creates a new Ethernet frame from the given buffer
    pub fn new(buffer: B) -> Self {
        assert!(buffer.as_slice().len() >= usize(HEADER_SIZE));

        Frame { buffer }
    }

    /// Parses bytes into an Ethernet frame
    pub fn parse(bytes: B) -> Result<Self, B> {
        if bytes.as_slice().len() < usize(HEADER_SIZE) {
            Err(bytes)
        } else {
            Ok(Frame { buffer: bytes })
        }
    }

    /* Getters */
    /// Returns the Destination field of the header
    pub fn get_destination(&self) -> mac::Addr {
        mac::Addr(*array_ref!(
            self.as_slice(),
            DESTINATION.start,
            DESTINATION.end - DESTINATION.start
        ))
    }

    /// Returns the Source field of the header
    pub fn get_source(&self) -> mac::Addr {
        mac::Addr(*array_ref!(
            self.as_slice(),
            SOURCE.start,
            SOURCE.end - SOURCE.start
        ))
    }

    /// Returns the Type field of the header
    pub fn get_type(&self) -> Type {
        NE::read_u16(&self.as_slice()[TYPE]).into()
    }

    /// View into the payload
    pub fn payload(&self) -> &[u8] {
        &self.as_slice()[PAYLOAD]
    }

    /* Miscellaneous */
    /// Returns the byte representation of this frame
    pub fn as_bytes(&self) -> &[u8] {
        self.as_slice()
    }

    /// Frees the underlying buffer
    pub fn free(self) -> B {
        self.buffer
    }

    /// Returns the length (header + data) of this frame
    pub fn len(&self) -> u16 {
        u16(self.as_bytes().len()).unwrap()
    }

    /* Private */
    fn as_slice(&self) -> &[u8] {
        self.buffer.as_slice()
    }
}

impl<B> Frame<B>
where
    B: AsSlice<Element=u8> + AsMutSlice<Element=u8>,
{
    /* Setters */
    /// Sets the destination field of the header
    pub fn set_destination(&mut self, addr: mac::Addr) {
        self.as_mut_slice()[DESTINATION].copy_from_slice(&addr.0)
    }

    /// Sets the source field of the header
    pub fn set_source(&mut self, addr: mac::Addr) {
        self.as_mut_slice()[SOURCE].copy_from_slice(&addr.0)
    }

    /// Sets the type field of the header
    pub fn set_type(&mut self, type_: Type) {
        NE::write_u16(&mut self.as_mut_slice()[TYPE], type_.into())
    }

    /* Miscellaneous */
    /// Mutable view into the payload
    pub fn payload_mut(&mut self) -> &mut [u8] {
        &mut self.as_mut_slice()[PAYLOAD]
    }

    /* Private */
    fn as_mut_slice(&mut self) -> &mut [u8] {
        self.buffer.as_mut_slice()
    }
}

impl<B> Frame<B>
where
    B: AsSlice<Element=u8> + Resize,
{
    /// Truncates the *payload* of this frame to the specified length
    pub fn truncate(&mut self, len: u16) {
        self.buffer.truncate(len + HEADER_SIZE);
    }

    /// Returns the payload of this frame
    pub fn into_payload(self) -> B {
        let mut buffer = self.buffer;
        buffer.slice_from(HEADER_SIZE);
        buffer
    }
}

impl<B> Frame<B>
where
    B: AsSlice<Element=u8> + AsMutSlice<Element=u8> + Resize,
{
    /// Fills the payload with an ARP packet
    ///
    /// This method sets the Type field of this frame to ARP, and truncates the length of the frame
    /// to fit the ARP packet.
    ///
    /// The ARP packet will have its SHA set to the Ethernet frame Source address
    pub fn arp<F>(&mut self, f: F)
    where
        F: FnOnce(&mut arp::Packet<&mut [u8]>),
    {
        self.set_type(Type::Arp);
        let sha = self.get_source();
        let len = {
            let mut arp = arp::Packet::new(self.payload_mut());
            arp.set_sha(sha);
            f(&mut arp);
            arp.len()
        };
        self.truncate(len);
    }

    /// Fills the payload with an IPv4 packet
    ///
    /// This method sets the Type field of this frame to IPv4, recomputes and updates the header
    /// checksum of the IPv4 payload, and truncates the length of the frame to fit the IPv4 packet.
    pub fn ipv4<F>(&mut self, f: F)
    where
        F: FnOnce(&mut ipv4::Packet<&mut [u8], Invalid>),
    {
        self.set_type(Type::Ipv4);
        let len = {
            let mut ip = ipv4::Packet::new(self.payload_mut());
            f(&mut ip);
            ip.update_checksum().get_total_length()
        };
        self.truncate(len);
    }
}

/// NOTE excludes the payload
impl<B> fmt::Debug for Frame<B>
where
    B: AsSlice<Element=u8>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("ether::Frame")
            .field("destination", &self.get_destination())
            .field("source", &self.get_source())
            .field("type", &self.get_type())
            // .field("payload", &self.payload())
            .finish()
    }
}

full_range!(u16,
/// Ether Type
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Type {
    /// IPv4
    Ipv4 = 0x0800,
    /// ARP
    Arp = 0x0806,
    /// IPv6
    Ipv6 = 0x08DD,
}
);

#[cfg(test)]
mod tests {
    use {ether, Buffer};

    #[test]
    fn new() {
        const SZ: u16 = 128;

        let mut chunk = [0; SZ as usize];
        let buf = Buffer::new(&mut chunk);

        let eth = ether::Frame::new(buf);
        assert_eq!(eth.len(), SZ);
    }
}
