//! Non-blocking socket primitives and readiness polling over the platform socket API (BSD
//! sockets on Unix, Winsock on Windows), used by the event loop in place of libuv's.

use std::io;
use std::mem::{size_of, zeroed};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

#[cfg(unix)]
pub type RawSock = libc::c_int;
#[cfg(windows)]
pub type RawSock = windows_sys::Win32::Networking::WinSock::SOCKET;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    V4,
    V6,
}

impl Family {
    pub fn of(addr: &SocketAddr) -> Family {
        if addr.is_ipv4() { Family::V4 } else { Family::V6 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Stream,
    Datagram,
}

/// Ensures the platform socket library is initialized (Winsock requires `WSAStartup`, which the
/// standard library performs when it creates its first socket).
pub fn init() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        // Creating (and dropping) a socket through the standard library initializes Winsock.
        let _ = std::net::UdpSocket::bind((Ipv4Addr::LOCALHOST, 0));
    });
}

fn last_error() -> io::Error {
    #[cfg(unix)]
    {
        io::Error::last_os_error()
    }
    #[cfg(windows)]
    {
        io::Error::from_raw_os_error(unsafe { windows_sys::Win32::Networking::WinSock::WSAGetLastError() })
    }
}

// ---------------------------------------------------------------------------------------------
// Socket addresses
// ---------------------------------------------------------------------------------------------

#[cfg(unix)]
mod addr {
    use super::*;

    pub type Storage = libc::sockaddr_storage;
    pub type Len = libc::socklen_t;

    pub fn to_storage(addr: &SocketAddr) -> (Storage, Len) {
        let mut storage: Storage = unsafe { zeroed() };
        match addr {
            SocketAddr::V4(a) => {
                let sin = &mut storage as *mut Storage as *mut libc::sockaddr_in;
                unsafe {
                    (*sin).sin_family = libc::AF_INET as libc::sa_family_t;
                    (*sin).sin_port = a.port().to_be();
                    (*sin).sin_addr = libc::in_addr { s_addr: u32::from_ne_bytes(a.ip().octets()) };
                    #[cfg(any(
                        target_os = "macos",
                        target_os = "ios",
                        target_os = "freebsd",
                        target_os = "netbsd",
                        target_os = "openbsd",
                        target_os = "dragonfly"
                    ))]
                    {
                        (*sin).sin_len = size_of::<libc::sockaddr_in>() as u8;
                    }
                }
                (storage, size_of::<libc::sockaddr_in>() as Len)
            }
            SocketAddr::V6(a) => {
                let sin6 = &mut storage as *mut Storage as *mut libc::sockaddr_in6;
                unsafe {
                    (*sin6).sin6_family = libc::AF_INET6 as libc::sa_family_t;
                    (*sin6).sin6_port = a.port().to_be();
                    (*sin6).sin6_addr = libc::in6_addr { s6_addr: a.ip().octets() };
                    (*sin6).sin6_flowinfo = a.flowinfo();
                    (*sin6).sin6_scope_id = a.scope_id();
                    #[cfg(any(
                        target_os = "macos",
                        target_os = "ios",
                        target_os = "freebsd",
                        target_os = "netbsd",
                        target_os = "openbsd",
                        target_os = "dragonfly"
                    ))]
                    {
                        (*sin6).sin6_len = size_of::<libc::sockaddr_in6>() as u8;
                    }
                }
                (storage, size_of::<libc::sockaddr_in6>() as Len)
            }
        }
    }

    pub unsafe fn from_raw(p: *const libc::sockaddr) -> Option<SocketAddr> {
        unsafe {
            match (*p).sa_family as i32 {
                libc::AF_INET => {
                    let sin = &*(p as *const libc::sockaddr_in);
                    Some(SocketAddr::V4(SocketAddrV4::new(
                        Ipv4Addr::from(sin.sin_addr.s_addr.to_ne_bytes()),
                        u16::from_be(sin.sin_port),
                    )))
                }
                libc::AF_INET6 => {
                    let sin6 = &*(p as *const libc::sockaddr_in6);
                    Some(SocketAddr::V6(SocketAddrV6::new(
                        Ipv6Addr::from(sin6.sin6_addr.s6_addr),
                        u16::from_be(sin6.sin6_port),
                        sin6.sin6_flowinfo,
                        sin6.sin6_scope_id,
                    )))
                }
                _ => None,
            }
        }
    }
}

#[cfg(windows)]
mod addr {
    use super::*;
    use windows_sys::Win32::Networking::WinSock::*;

    pub type Storage = SOCKADDR_STORAGE;
    pub type Len = i32;

    pub fn to_storage(addr: &SocketAddr) -> (Storage, Len) {
        let mut storage: Storage = unsafe { zeroed() };
        match addr {
            SocketAddr::V4(a) => {
                let sin = &mut storage as *mut Storage as *mut SOCKADDR_IN;
                unsafe {
                    (*sin).sin_family = AF_INET;
                    (*sin).sin_port = a.port().to_be();
                    (*sin).sin_addr.S_un.S_addr = u32::from_ne_bytes(a.ip().octets());
                }
                (storage, size_of::<SOCKADDR_IN>() as Len)
            }
            SocketAddr::V6(a) => {
                let sin6 = &mut storage as *mut Storage as *mut SOCKADDR_IN6;
                unsafe {
                    (*sin6).sin6_family = AF_INET6;
                    (*sin6).sin6_port = a.port().to_be();
                    (*sin6).sin6_addr.u.Byte = a.ip().octets();
                    (*sin6).sin6_flowinfo = a.flowinfo();
                    (*sin6).Anonymous.sin6_scope_id = a.scope_id();
                }
                (storage, size_of::<SOCKADDR_IN6>() as Len)
            }
        }
    }

    pub unsafe fn from_raw(p: *const SOCKADDR) -> Option<SocketAddr> {
        unsafe {
            match (*p).sa_family {
                AF_INET => {
                    let sin = &*(p as *const SOCKADDR_IN);
                    Some(SocketAddr::V4(SocketAddrV4::new(
                        Ipv4Addr::from(sin.sin_addr.S_un.S_addr.to_ne_bytes()),
                        u16::from_be(sin.sin_port),
                    )))
                }
                AF_INET6 => {
                    let sin6 = &*(p as *const SOCKADDR_IN6);
                    Some(SocketAddr::V6(SocketAddrV6::new(
                        Ipv6Addr::from(sin6.sin6_addr.u.Byte),
                        u16::from_be(sin6.sin6_port),
                        sin6.sin6_flowinfo,
                        sin6.Anonymous.sin6_scope_id,
                    )))
                }
                _ => None,
            }
        }
    }
}

pub use addr::from_raw as sockaddr_from_raw;
pub use addr::to_storage as sockaddr_storage_of;

// ---------------------------------------------------------------------------------------------
// Sockets
// ---------------------------------------------------------------------------------------------

/// An owned, non-blocking, non-inheritable socket. Dropping it closes the socket.
pub struct Sock {
    raw: RawSock,
    pub family: Family,
}

/// Socket option levels and names used by the event loop.
#[derive(Clone, Copy)]
pub enum Opt {
    ReuseAddr,
    #[cfg_attr(
        not(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly"
        )),
        allow(dead_code)
    )]
    ReusePort,
    KeepAlive,
    NoDelay,
    KeepIdle,
    Broadcast,
    Ipv6Only,
    Ttl,
    MulticastTtl,
    MulticastLoop,
    Ipv6UnicastHops,
    Ipv6MulticastHops,
    Ipv6MulticastLoop,
}

#[cfg(unix)]
fn opt_level_name(opt: Opt) -> (libc::c_int, libc::c_int) {
    match opt {
        Opt::ReuseAddr => (libc::SOL_SOCKET, libc::SO_REUSEADDR),
        Opt::ReusePort => (libc::SOL_SOCKET, libc::SO_REUSEPORT),
        Opt::KeepAlive => (libc::SOL_SOCKET, libc::SO_KEEPALIVE),
        Opt::NoDelay => (libc::IPPROTO_TCP, libc::TCP_NODELAY),
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        Opt::KeepIdle => (libc::IPPROTO_TCP, libc::TCP_KEEPALIVE),
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        Opt::KeepIdle => (libc::IPPROTO_TCP, libc::TCP_KEEPIDLE),
        Opt::Broadcast => (libc::SOL_SOCKET, libc::SO_BROADCAST),
        Opt::Ipv6Only => (libc::IPPROTO_IPV6, libc::IPV6_V6ONLY),
        Opt::Ttl => (libc::IPPROTO_IP, libc::IP_TTL),
        Opt::MulticastTtl => (libc::IPPROTO_IP, libc::IP_MULTICAST_TTL),
        Opt::MulticastLoop => (libc::IPPROTO_IP, libc::IP_MULTICAST_LOOP),
        Opt::Ipv6UnicastHops => (libc::IPPROTO_IPV6, libc::IPV6_UNICAST_HOPS),
        Opt::Ipv6MulticastHops => (libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_HOPS),
        Opt::Ipv6MulticastLoop => (libc::IPPROTO_IPV6, libc::IPV6_MULTICAST_LOOP),
    }
}

#[cfg(windows)]
fn opt_level_name(opt: Opt) -> (i32, i32) {
    use windows_sys::Win32::Networking::WinSock::*;
    match opt {
        Opt::ReuseAddr | Opt::ReusePort => (SOL_SOCKET, SO_REUSEADDR),
        Opt::KeepAlive => (SOL_SOCKET, SO_KEEPALIVE),
        Opt::NoDelay => (IPPROTO_TCP, TCP_NODELAY),
        Opt::KeepIdle => (IPPROTO_TCP, TCP_KEEPIDLE),
        Opt::Broadcast => (SOL_SOCKET, SO_BROADCAST),
        Opt::Ipv6Only => (IPPROTO_IPV6, IPV6_V6ONLY),
        Opt::Ttl => (IPPROTO_IP, IP_TTL),
        Opt::MulticastTtl => (IPPROTO_IP, IP_MULTICAST_TTL),
        Opt::MulticastLoop => (IPPROTO_IP, IP_MULTICAST_LOOP),
        Opt::Ipv6UnicastHops => (IPPROTO_IPV6, IPV6_UNICAST_HOPS),
        Opt::Ipv6MulticastHops => (IPPROTO_IPV6, IPV6_MULTICAST_HOPS),
        Opt::Ipv6MulticastLoop => (IPPROTO_IPV6, IPV6_MULTICAST_LOOP),
    }
}

impl Sock {
    pub fn raw(&self) -> RawSock {
        self.raw
    }

    #[cfg(unix)]
    pub fn new(family: Family, kind: Kind) -> io::Result<Sock> {
        let domain = match family {
            Family::V4 => libc::AF_INET,
            Family::V6 => libc::AF_INET6,
        };
        let ty = match kind {
            Kind::Stream => libc::SOCK_STREAM,
            Kind::Datagram => libc::SOCK_DGRAM,
        };
        let raw = unsafe { libc::socket(domain, ty, 0) };
        if raw < 0 {
            return Err(last_error());
        }
        let sock = Sock { raw, family };
        sock.set_nonblocking_cloexec()?;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        sock.set_int(libc::SOL_SOCKET, libc::SO_NOSIGPIPE, 1)?;
        Ok(sock)
    }

    #[cfg(windows)]
    pub fn new(family: Family, kind: Kind) -> io::Result<Sock> {
        use windows_sys::Win32::Networking::WinSock::*;
        init();
        let domain = match family {
            Family::V4 => AF_INET as i32,
            Family::V6 => AF_INET6 as i32,
        };
        let (ty, proto) = match kind {
            Kind::Stream => (SOCK_STREAM, IPPROTO_TCP),
            Kind::Datagram => (SOCK_DGRAM, IPPROTO_UDP),
        };
        let raw = unsafe {
            WSASocketW(domain, ty, proto, std::ptr::null(), 0, WSA_FLAG_OVERLAPPED | WSA_FLAG_NO_HANDLE_INHERIT)
        };
        if raw == INVALID_SOCKET {
            return Err(last_error());
        }
        let sock = Sock { raw, family };
        sock.set_nonblocking_cloexec()?;
        Ok(sock)
    }

    #[cfg(unix)]
    fn set_nonblocking_cloexec(&self) -> io::Result<()> {
        unsafe {
            let flags = libc::fcntl(self.raw, libc::F_GETFL);
            if flags < 0 || libc::fcntl(self.raw, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(last_error());
            }
            let fd_flags = libc::fcntl(self.raw, libc::F_GETFD);
            if fd_flags < 0 || libc::fcntl(self.raw, libc::F_SETFD, fd_flags | libc::FD_CLOEXEC) < 0 {
                return Err(last_error());
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    fn set_nonblocking_cloexec(&self) -> io::Result<()> {
        use windows_sys::Win32::Networking::WinSock::*;
        let mut on: u32 = 1;
        if unsafe { ioctlsocket(self.raw, FIONBIO, &mut on) } != 0 {
            return Err(last_error());
        }
        Ok(())
    }

    #[cfg(unix)]
    pub fn set_int(&self, level: libc::c_int, name: libc::c_int, value: libc::c_int) -> io::Result<()> {
        let r = unsafe {
            libc::setsockopt(
                self.raw,
                level,
                name,
                &value as *const libc::c_int as *const libc::c_void,
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if r != 0 { Err(last_error()) } else { Ok(()) }
    }

    #[cfg(windows)]
    pub fn set_int(&self, level: i32, name: i32, value: i32) -> io::Result<()> {
        use windows_sys::Win32::Networking::WinSock::*;
        let r =
            unsafe { setsockopt(self.raw, level, name, &value as *const i32 as *const u8, size_of::<i32>() as i32) };
        if r != 0 { Err(last_error()) } else { Ok(()) }
    }

    pub fn set_opt(&self, opt: Opt, value: i32) -> io::Result<()> {
        let (level, name) = opt_level_name(opt);
        self.set_int(level, name, value)
    }

    /// Sets a raw option value (multicast membership and interface structures).
    pub fn set_raw(&self, level: i32, name: i32, value: &[u8]) -> io::Result<()> {
        #[cfg(unix)]
        let r = unsafe {
            libc::setsockopt(
                self.raw,
                level,
                name,
                value.as_ptr() as *const libc::c_void,
                value.len() as libc::socklen_t,
            )
        };
        #[cfg(windows)]
        let r = unsafe {
            windows_sys::Win32::Networking::WinSock::setsockopt(
                self.raw,
                level,
                name,
                value.as_ptr(),
                value.len() as i32,
            )
        };
        if r != 0 { Err(last_error()) } else { Ok(()) }
    }

    pub fn bind(&self, addr: &SocketAddr) -> io::Result<()> {
        let (storage, len) = addr::to_storage(addr);
        #[cfg(unix)]
        let r = unsafe { libc::bind(self.raw, &storage as *const _ as *const libc::sockaddr, len) };
        #[cfg(windows)]
        let r =
            unsafe { windows_sys::Win32::Networking::WinSock::bind(self.raw, &storage as *const _ as *const _, len) };
        if r != 0 { Err(last_error()) } else { Ok(()) }
    }

    pub fn listen(&self, backlog: i32) -> io::Result<()> {
        #[cfg(unix)]
        let r = unsafe { libc::listen(self.raw, backlog) };
        #[cfg(windows)]
        let r = unsafe { windows_sys::Win32::Networking::WinSock::listen(self.raw, backlog) };
        if r != 0 { Err(last_error()) } else { Ok(()) }
    }

    /// Starts a non-blocking connection. `Ok(false)` means the connection is in progress.
    pub fn connect(&self, addr: &SocketAddr) -> io::Result<bool> {
        let (storage, len) = addr::to_storage(addr);
        loop {
            #[cfg(unix)]
            let r = unsafe { libc::connect(self.raw, &storage as *const _ as *const libc::sockaddr, len) };
            #[cfg(windows)]
            let r = unsafe {
                windows_sys::Win32::Networking::WinSock::connect(self.raw, &storage as *const _ as *const _, len)
            };
            if r == 0 {
                return Ok(true);
            }
            let e = last_error();
            match e.raw_os_error() {
                #[cfg(unix)]
                Some(libc::EINTR) => continue,
                #[cfg(unix)]
                Some(libc::EINPROGRESS) => return Ok(false),
                #[cfg(windows)]
                Some(windows_sys::Win32::Networking::WinSock::WSAEWOULDBLOCK) => return Ok(false),
                _ => return Err(e),
            }
        }
    }

    pub fn accept(&self) -> io::Result<Sock> {
        loop {
            let mut storage: addr::Storage = unsafe { zeroed() };
            let mut len = size_of::<addr::Storage>() as addr::Len;
            #[cfg(unix)]
            let raw = unsafe { libc::accept(self.raw, &mut storage as *mut _ as *mut libc::sockaddr, &mut len) };
            #[cfg(windows)]
            let raw = unsafe {
                windows_sys::Win32::Networking::WinSock::accept(self.raw, &mut storage as *mut _ as *mut _, &mut len)
            };
            #[cfg(unix)]
            let failed = raw < 0;
            #[cfg(windows)]
            let failed = raw == windows_sys::Win32::Networking::WinSock::INVALID_SOCKET;
            if failed {
                let e = last_error();
                #[cfg(unix)]
                if e.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(e);
            }
            let family = match unsafe { addr::from_raw(&storage as *const _ as *const _) } {
                Some(a) => Family::of(&a),
                None => self.family,
            };
            let sock = Sock { raw, family };
            sock.set_nonblocking_cloexec()?;
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            sock.set_int(libc::SOL_SOCKET, libc::SO_NOSIGPIPE, 1)?;
            return Ok(sock);
        }
    }

    /// The pending error of the socket (`SO_ERROR`), as a raw OS error code.
    pub fn take_error(&self) -> io::Result<i32> {
        let mut value: i32 = 0;
        #[cfg(unix)]
        let r = unsafe {
            let mut len = size_of::<i32>() as libc::socklen_t;
            libc::getsockopt(
                self.raw,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                &mut value as *mut i32 as *mut libc::c_void,
                &mut len,
            )
        };
        #[cfg(windows)]
        let r = unsafe {
            use windows_sys::Win32::Networking::WinSock::*;
            let mut len = size_of::<i32>() as i32;
            getsockopt(self.raw, SOL_SOCKET, SO_ERROR, &mut value as *mut i32 as *mut u8, &mut len)
        };
        if r != 0 { Err(last_error()) } else { Ok(value) }
    }

    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            #[cfg(unix)]
            let n = unsafe { libc::recv(self.raw, buf.as_mut_ptr() as *mut libc::c_void, buf.len(), 0) as isize };
            #[cfg(windows)]
            let n = unsafe {
                windows_sys::Win32::Networking::WinSock::recv(
                    self.raw,
                    buf.as_mut_ptr(),
                    buf.len().min(i32::MAX as usize) as i32,
                    0,
                ) as isize
            };
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = last_error();
            #[cfg(unix)]
            if e.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(e);
        }
    }

    /// Sends the concatenation of `bufs` with one system call (`writev` semantics).
    pub fn send_vectored(&self, bufs: &[&[u8]]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            let iov: Vec<libc::iovec> = bufs
                .iter()
                .map(|b| libc::iovec { iov_base: b.as_ptr() as *mut libc::c_void, iov_len: b.len() })
                .collect();
            #[cfg(any(target_os = "linux", target_os = "android"))]
            let flags = libc::MSG_NOSIGNAL;
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            let flags = 0;
            loop {
                let mut msg: libc::msghdr = unsafe { zeroed() };
                msg.msg_iov = iov.as_ptr() as *mut libc::iovec;
                msg.msg_iovlen = iov.len().min(libc::c_int::MAX as usize) as _;
                let n = unsafe { libc::sendmsg(self.raw, &msg, flags) };
                if n >= 0 {
                    return Ok(n as usize);
                }
                let e = last_error();
                if e.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                return Err(e);
            }
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::Networking::WinSock::*;
            let wsabufs: Vec<WSABUF> = bufs
                .iter()
                .map(|b| WSABUF { len: b.len().min(u32::MAX as usize) as u32, buf: b.as_ptr() as *mut u8 })
                .collect();
            let mut sent: u32 = 0;
            let r = unsafe {
                WSASend(self.raw, wsabufs.as_ptr(), wsabufs.len() as u32, &mut sent, 0, std::ptr::null_mut(), None)
            };
            if r != 0 {
                return Err(last_error());
            }
            Ok(sent as usize)
        }
    }

    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, Option<SocketAddr>)> {
        loop {
            let mut storage: addr::Storage = unsafe { zeroed() };
            let mut len = size_of::<addr::Storage>() as addr::Len;
            #[cfg(unix)]
            let n = unsafe {
                libc::recvfrom(
                    self.raw,
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                    &mut storage as *mut _ as *mut libc::sockaddr,
                    &mut len,
                ) as isize
            };
            #[cfg(windows)]
            let n = unsafe {
                windows_sys::Win32::Networking::WinSock::recvfrom(
                    self.raw,
                    buf.as_mut_ptr(),
                    buf.len().min(i32::MAX as usize) as i32,
                    0,
                    &mut storage as *mut _ as *mut _,
                    &mut len,
                ) as isize
            };
            if n >= 0 {
                let from = if len > 0 { unsafe { addr::from_raw(&storage as *const _ as *const _) } } else { None };
                return Ok((n as usize, from));
            }
            let e = last_error();
            #[cfg(unix)]
            if e.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            #[cfg(windows)]
            if e.raw_os_error() == Some(windows_sys::Win32::Networking::WinSock::WSAEMSGSIZE) {
                // A datagram larger than the buffer: the buffer holds its truncated prefix.
                return Ok((buf.len(), unsafe { addr::from_raw(&storage as *const _ as *const _) }));
            }
            return Err(e);
        }
    }

    pub fn send_to(&self, buf: &[u8], to: Option<&SocketAddr>) -> io::Result<usize> {
        let target = to.map(addr::to_storage);
        loop {
            #[cfg(unix)]
            let n = unsafe {
                match &target {
                    Some((storage, len)) => libc::sendto(
                        self.raw,
                        buf.as_ptr() as *const libc::c_void,
                        buf.len(),
                        0,
                        storage as *const _ as *const libc::sockaddr,
                        *len,
                    ),
                    None => libc::send(self.raw, buf.as_ptr() as *const libc::c_void, buf.len(), 0),
                }
            } as isize;
            #[cfg(windows)]
            let n = unsafe {
                use windows_sys::Win32::Networking::WinSock::*;
                let len = buf.len().min(i32::MAX as usize) as i32;
                match &target {
                    Some((storage, slen)) => {
                        sendto(self.raw, buf.as_ptr(), len, 0, storage as *const _ as *const _, *slen)
                    }
                    None => send(self.raw, buf.as_ptr(), len, 0),
                }
            } as isize;
            if n >= 0 {
                return Ok(n as usize);
            }
            let e = last_error();
            #[cfg(unix)]
            if e.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(e);
        }
    }

    pub fn shutdown_write(&self) -> io::Result<()> {
        #[cfg(unix)]
        let r = unsafe { libc::shutdown(self.raw, libc::SHUT_WR) };
        #[cfg(windows)]
        let r = unsafe {
            windows_sys::Win32::Networking::WinSock::shutdown(
                self.raw,
                windows_sys::Win32::Networking::WinSock::SD_SEND,
            )
        };
        if r != 0 { Err(last_error()) } else { Ok(()) }
    }

    fn name(&self, peer: bool) -> io::Result<SocketAddr> {
        let mut storage: addr::Storage = unsafe { zeroed() };
        let mut len = size_of::<addr::Storage>() as addr::Len;
        #[cfg(unix)]
        let r = unsafe {
            let p = &mut storage as *mut _ as *mut libc::sockaddr;
            if peer { libc::getpeername(self.raw, p, &mut len) } else { libc::getsockname(self.raw, p, &mut len) }
        };
        #[cfg(windows)]
        let r = unsafe {
            use windows_sys::Win32::Networking::WinSock::*;
            let p = &mut storage as *mut _ as *mut SOCKADDR;
            if peer { getpeername(self.raw, p, &mut len) } else { getsockname(self.raw, p, &mut len) }
        };
        if r != 0 {
            return Err(last_error());
        }
        unsafe { addr::from_raw(&storage as *const _ as *const _) }
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.name(false)
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.name(true)
    }
}

impl Drop for Sock {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::close(self.raw);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Networking::WinSock::closesocket(self.raw);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Readiness polling
// ---------------------------------------------------------------------------------------------

pub struct PollEntry {
    pub sock: RawSock,
    pub read: bool,
    pub write: bool,
    pub readable: bool,
    pub writable: bool,
}

/// Waits until one of `entries` is ready or `timeout_ms` elapses (`-1`: no timeout).
#[cfg(unix)]
pub fn poll(entries: &mut [PollEntry], timeout_ms: i32) -> io::Result<()> {
    let mut fds: Vec<libc::pollfd> = entries
        .iter()
        .map(|e| libc::pollfd {
            fd: e.sock,
            events: (if e.read { libc::POLLIN } else { 0 }) | (if e.write { libc::POLLOUT } else { 0 }),
            revents: 0,
        })
        .collect();
    let r = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
    if r < 0 {
        let e = last_error();
        if e.raw_os_error() == Some(libc::EINTR) {
            return Ok(());
        }
        return Err(e);
    }
    for (e, f) in entries.iter_mut().zip(&fds) {
        let err = f.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0;
        e.readable = e.read && (f.revents & libc::POLLIN != 0 || err);
        e.writable = e.write && (f.revents & libc::POLLOUT != 0 || err);
    }
    Ok(())
}

/// Waits until one of `entries` is ready or `timeout_ms` elapses (`-1`: no timeout).
#[cfg(windows)]
pub fn poll(entries: &mut [PollEntry], timeout_ms: i32) -> io::Result<()> {
    use windows_sys::Win32::Networking::WinSock::*;
    let mut fds: Vec<WSAPOLLFD> = entries
        .iter()
        .map(|e| WSAPOLLFD {
            fd: e.sock,
            events: (if e.read { POLLRDNORM } else { 0 }) | (if e.write { POLLWRNORM } else { 0 }),
            revents: 0,
        })
        .collect();
    let r = unsafe { WSAPoll(fds.as_mut_ptr(), fds.len() as u32, timeout_ms) };
    if r < 0 {
        return Err(last_error());
    }
    for (e, f) in entries.iter_mut().zip(&fds) {
        let err = f.revents & (POLLERR | POLLHUP | POLLNVAL) != 0;
        e.readable = e.read && (f.revents & (POLLRDNORM | POLLRDBAND) != 0 || err);
        e.writable = e.write && (f.revents & POLLWRNORM != 0 || err);
    }
    Ok(())
}
