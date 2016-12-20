use Error;
use Managed;
use wire::{IpProtocol, IpAddress, IpEndpoint};
use wire::{TcpPacket, TcpRepr, TcpControl};
use socket::{Socket};

/// A TCP stream ring buffer.
#[derive(Debug)]
pub struct SocketBuffer<'a> {
    storage: Managed<'a, [u8]>,
    read_at: usize,
    length:  usize
}

impl<'a> SocketBuffer<'a> {
    /// Create a packet buffer with the given storage.
    pub fn new<T>(storage: T) -> SocketBuffer<'a>
            where T: Into<Managed<'a, [u8]>> {
        SocketBuffer {
            storage: storage.into(),
            read_at: 0,
            length:  0
        }
    }

    /// Enqueue a slice of octets up to the given size into the buffer, and return a pointer
    /// to the slice.
    ///
    /// The returned slice may be shorter than requested, as short as an empty slice,
    /// if there is not enough contiguous free space in the buffer.
    pub fn enqueue(&mut self, mut size: usize) -> &mut [u8] {
        let write_at = (self.read_at + self.length) % self.storage.len();
        // We can't enqueue more than there is free space.
        let free = self.storage.len() - self.length;
        if size > free { size = free }
        // We can't contiguously enqueue past the beginning of the storage.
        let until_end = self.storage.len() - write_at;
        if size > until_end { size = until_end }

        self.length += size;
        &mut self.storage[write_at..write_at + size]
    }

    /// Dequeue a slice of octets up to the given size from the buffer, and return a pointer
    /// to the slice.
    ///
    /// The returned slice may be shorter than requested, as short as an empty slice,
    /// if there is not enough contiguous filled space in the buffer.
    pub fn dequeue(&mut self, mut size: usize) -> &[u8] {
        let read_at = self.read_at;
        // We can't dequeue more than was queued.
        if size > self.length { size = self.length }
        // We can't contiguously dequeue past the end of the storage.
        let until_end = self.storage.len() - self.read_at;
        if size > until_end { size = until_end }

        self.read_at = (self.read_at + size) % self.storage.len();
        self.length -= size;
        &self.storage[read_at..read_at + size]
    }
}

/// A description of incoming TCP connection.
#[derive(Debug)]
pub struct Incoming {
    local_end:  IpEndpoint,
    remote_end: IpEndpoint,
    seq_number: u32
}

impl Incoming {
    /// Return the local endpoint.
    pub fn local_end(&self) -> IpEndpoint {
        self.local_end
    }

    /// Return the remote endpoint.
    pub fn remote_end(&self) -> IpEndpoint {
        self.remote_end
    }
}

/// A Transmission Control Protocol server socket.
#[derive(Debug)]
pub struct Listener<'a> {
    endpoint:   IpEndpoint,
    backlog:    Managed<'a, [Option<Incoming>]>,
    accept_at:  usize,
    length:     usize
}

impl<'a> Listener<'a> {
    /// Create a server socket with the given backlog.
    pub fn new<T>(endpoint: IpEndpoint, backlog: T) -> Socket<'a, 'static>
            where T: Into<Managed<'a, [Option<Incoming>]>> {
        Socket::TcpServer(Listener {
            endpoint:  endpoint,
            backlog:   backlog.into(),
            accept_at: 0,
            length:    0
        })
    }

    /// Accept a connection from this server socket,
    pub fn accept(&mut self) -> Option<Incoming> {
        if self.length == 0 { return None }

        let accept_at = self.accept_at;
        self.accept_at = (self.accept_at + 1) % self.backlog.len();
        self.length -= 1;

        self.backlog[accept_at].take()
    }

    /// See [Socket::collect](enum.Socket.html#method.collect).
    pub fn collect(&mut self, src_addr: &IpAddress, dst_addr: &IpAddress,
                   protocol: IpProtocol, payload: &[u8])
            -> Result<(), Error> {
        if protocol != IpProtocol::Tcp { return Err(Error::Rejected) }

        let packet = try!(TcpPacket::new(payload));
        let repr = try!(TcpRepr::parse(&packet, src_addr, dst_addr));

        if repr.dst_port != self.endpoint.port { return Err(Error::Rejected) }
        if !self.endpoint.addr.is_unspecified() {
            if self.endpoint.addr != *dst_addr { return Err(Error::Rejected) }
        }

        match (repr.control, repr.ack_number) {
            (TcpControl::Syn, None) => {
                if self.length == self.backlog.len() { return Err(Error::Exhausted) }

                let inject_at = (self.accept_at + self.length) % self.backlog.len();
                self.length += 1;

                assert!(self.backlog[inject_at].is_none());
                self.backlog[inject_at] = Some(Incoming {
                    local_end:  IpEndpoint::new(*dst_addr, repr.dst_port),
                    remote_end: IpEndpoint::new(*src_addr, repr.src_port),
                    seq_number: repr.seq_number
                });
                Ok(())
            }
            _ => Err(Error::Rejected)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_buffer() {
        let mut buffer = SocketBuffer::new(vec![0; 8]);       // ........
        buffer.enqueue(6).copy_from_slice(b"foobar");   // foobar..
        assert_eq!(buffer.dequeue(3), b"foo");          // ...bar..
        buffer.enqueue(6).copy_from_slice(b"ba");       // ...barba
        buffer.enqueue(4).copy_from_slice(b"zho");      // zhobarba
        assert_eq!(buffer.dequeue(6), b"barba");        // zho.....
        assert_eq!(buffer.dequeue(8), b"zho");          // ........
        buffer.enqueue(8).copy_from_slice(b"gefug");    // ...gefug
    }
}
