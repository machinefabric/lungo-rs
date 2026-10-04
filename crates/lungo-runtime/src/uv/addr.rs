//! Conversions between Lean's `Std.Net` address values and socket addresses, and the address
//! parsing and formatting of libuv (`uv_inet_pton`, `uv_inet_ntop`), ported from
//! `runtime/uv/net_addr.cpp` and libuv's `inet.c`.
//!
//! Representations: `IPv4Addr` and `IPv6Addr` are arrays of boxed octets / 16-bit segments,
//! `IPAddr` and `SocketAddress` are constructors `v4` (tag 0) and `v6` (tag 1) with one field,
//! and `SocketAddressV4`/`V6` hold the address and a `UInt16` port.

use crate::object::*;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

// ---------------------------------------------------------------------------------------------
// Lean values
// ---------------------------------------------------------------------------------------------

pub unsafe fn ipv4_of_lean(a: Obj) -> Ipv4Addr {
    unsafe {
        let mut o = [0u8; 4];
        for (i, b) in o.iter_mut().enumerate() {
            *b = lean_unbox(lean_array_get_core(a, i)) as u8;
        }
        Ipv4Addr::from(o)
    }
}

pub unsafe fn ipv6_of_lean(a: Obj) -> Ipv6Addr {
    unsafe {
        let mut s = [0u16; 8];
        for (i, w) in s.iter_mut().enumerate() {
            *w = lean_unbox(lean_array_get_core(a, i)) as u16;
        }
        Ipv6Addr::from(s)
    }
}

/// The `IPAddr` value `ip` (borrowed).
pub unsafe fn ip_of_lean(ip: Obj) -> IpAddr {
    unsafe {
        let inner = lean_ctor_get(ip, 0);
        if lean_ptr_tag(ip) == 0 { IpAddr::V4(ipv4_of_lean(inner)) } else { IpAddr::V6(ipv6_of_lean(inner)) }
    }
}

/// The `SocketAddress` value `sa` (borrowed).
pub unsafe fn socket_addr_of_lean(sa: Obj) -> SocketAddr {
    unsafe {
        let inner = lean_ctor_get(sa, 0);
        let ip = lean_ctor_get(inner, 0);
        let port = lean_ctor_get_uint16(inner, size_of::<Obj>());
        if lean_ptr_tag(sa) == 0 {
            SocketAddr::V4(SocketAddrV4::new(ipv4_of_lean(ip), port))
        } else {
            SocketAddr::V6(SocketAddrV6::new(ipv6_of_lean(ip), port, 0, 0))
        }
    }
}

unsafe fn array_of(items: &[usize]) -> Obj {
    unsafe {
        let a = lean_alloc_array(items.len(), items.len());
        for (i, v) in items.iter().enumerate() {
            lean_array_set_core(a, i, lean_box(*v));
        }
        a
    }
}

pub unsafe fn lean_of_ipv4(ip: &Ipv4Addr) -> Obj {
    let o = ip.octets();
    unsafe { array_of(&o.map(|b| b as usize)) }
}

pub unsafe fn lean_of_ipv6(ip: &Ipv6Addr) -> Obj {
    let s = ip.segments();
    unsafe { array_of(&s.map(|w| w as usize)) }
}

pub unsafe fn lean_of_ip(ip: &IpAddr) -> Obj {
    unsafe {
        let (tag, inner) = match ip {
            IpAddr::V4(a) => (0, lean_of_ipv4(a)),
            IpAddr::V6(a) => (1, lean_of_ipv6(a)),
        };
        let r = lean_alloc_ctor(tag, 1, 0);
        lean_ctor_set(r, 0, inner);
        r
    }
}

pub unsafe fn lean_of_socket_addr(sa: &SocketAddr) -> Obj {
    unsafe {
        let (tag, ip) = match sa {
            SocketAddr::V4(a) => (0, lean_of_ipv4(a.ip())),
            SocketAddr::V6(a) => (1, lean_of_ipv6(a.ip())),
        };
        let inner = lean_alloc_ctor(0, 1, 2);
        lean_ctor_set(inner, 0, ip);
        lean_ctor_set_uint16(inner, size_of::<Obj>(), sa.port());
        let r = lean_alloc_ctor(tag, 1, 0);
        lean_ctor_set(r, 0, inner);
        r
    }
}

pub unsafe fn lean_of_mac(mac: &[u8; 6]) -> Obj {
    unsafe { array_of(&mac.map(|b| b as usize)) }
}

// ---------------------------------------------------------------------------------------------
// libuv's inet_pton / inet_ntop
// ---------------------------------------------------------------------------------------------

/// `inet_pton4`: exactly four decimal octets without leading zeros.
pub fn pton4(src: &[u8]) -> Option<[u8; 4]> {
    let mut tmp = [0u8; 4];
    let mut tp = 0usize;
    let mut saw_digit = false;
    let mut octets = 0;
    for &ch in src {
        if ch.is_ascii_digit() {
            let nw = tmp[tp] as u32 * 10 + (ch - b'0') as u32;
            if saw_digit && tmp[tp] == 0 {
                return None;
            }
            if nw > 255 {
                return None;
            }
            tmp[tp] = nw as u8;
            if !saw_digit {
                octets += 1;
                if octets > 4 {
                    return None;
                }
                saw_digit = true;
            }
        } else if ch == b'.' && saw_digit {
            if octets == 4 {
                return None;
            }
            tp += 1;
            tmp[tp] = 0;
            saw_digit = false;
        } else {
            return None;
        }
    }
    if octets < 4 {
        return None;
    }
    Some(tmp)
}

/// `inet_pton6`, including embedded IPv4 notation.
pub fn pton6(src: &[u8]) -> Option<[u8; 16]> {
    let mut tmp = [0u8; 16];
    let mut tp = 0usize;
    let endp = 16usize;
    let mut colonp: Option<usize> = None;
    let mut s = 0usize;
    if src.first() == Some(&b':') {
        s += 1;
        if src.get(s) != Some(&b':') {
            return None;
        }
    }
    let mut curtok = s;
    let mut seen_xdigits = 0;
    let mut val: u32 = 0;
    while s < src.len() {
        let ch = src[s];
        s += 1;
        if let Some(d) = (ch as char).to_digit(16) {
            val = (val << 4) | d;
            seen_xdigits += 1;
            if seen_xdigits > 4 {
                return None;
            }
            continue;
        }
        if ch == b':' {
            curtok = s;
            if seen_xdigits == 0 {
                if colonp.is_some() {
                    return None;
                }
                colonp = Some(tp);
                continue;
            } else if s == src.len() {
                return None;
            }
            if tp + 2 > endp {
                return None;
            }
            tmp[tp] = ((val >> 8) & 0xff) as u8;
            tmp[tp + 1] = (val & 0xff) as u8;
            tp += 2;
            seen_xdigits = 0;
            val = 0;
            continue;
        }
        if ch == b'.'
            && tp + 4 <= endp
            && let Some(v4) = pton4(&src[curtok..])
        {
            tmp[tp..tp + 4].copy_from_slice(&v4);
            tp += 4;
            seen_xdigits = 0;
            s = src.len();
            break;
        }
        return None;
    }
    if seen_xdigits > 0 {
        if tp + 2 > endp {
            return None;
        }
        tmp[tp] = ((val >> 8) & 0xff) as u8;
        tmp[tp + 1] = (val & 0xff) as u8;
        tp += 2;
    }
    let _ = s;
    if let Some(cp) = colonp {
        let n = tp - cp;
        if tp == endp {
            return None;
        }
        for i in 1..=n {
            tmp[endp - i] = tmp[cp + n - i];
            tmp[cp + n - i] = 0;
        }
        tp = endp;
    }
    if tp != endp {
        return None;
    }
    Some(tmp)
}

/// `uv_inet_pton(AF_INET6, …)`: a trailing `%zone` is ignored.
pub fn uv_pton6(src: &[u8]) -> Option<[u8; 16]> {
    const UV_INET6_ADDRSTRLEN: usize = 46;
    match src.iter().position(|&c| c == b'%') {
        Some(p) => {
            if p > UV_INET6_ADDRSTRLEN - 1 {
                return None;
            }
            pton6(&src[..p])
        }
        None => pton6(src),
    }
}

pub fn ntop4(o: &[u8; 4]) -> String {
    format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3])
}

/// `inet_ntop6`: the longest run of at least two zero words is compressed, and addresses of the
/// IPv4-compatible and IPv4-mapped forms end in dotted IPv4 notation.
pub fn ntop6(src: &[u8; 16]) -> String {
    let mut words = [0u32; 8];
    for (i, b) in src.iter().enumerate() {
        words[i / 2] |= (*b as u32) << ((1 - (i % 2)) << 3);
    }
    let (mut best_base, mut best_len) = (-1i32, 0i32);
    let (mut cur_base, mut cur_len) = (-1i32, 0i32);
    for (i, w) in words.iter().enumerate() {
        if *w == 0 {
            if cur_base == -1 {
                cur_base = i as i32;
                cur_len = 1;
            } else {
                cur_len += 1;
            }
        } else if cur_base != -1 {
            if best_base == -1 || cur_len > best_len {
                best_base = cur_base;
                best_len = cur_len;
            }
            cur_base = -1;
        }
    }
    if cur_base != -1 && (best_base == -1 || cur_len > best_len) {
        best_base = cur_base;
        best_len = cur_len;
    }
    if best_base != -1 && best_len < 2 {
        best_base = -1;
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i < 8 {
        let ii = i as i32;
        if best_base != -1 && ii >= best_base && ii < best_base + best_len {
            if ii == best_base {
                out.push(':');
            }
            i += 1;
            continue;
        }
        if i != 0 {
            out.push(':');
        }
        if i == 6
            && best_base == 0
            && (best_len == 6 || (best_len == 7 && words[7] != 0x0001) || (best_len == 5 && words[5] == 0xffff))
        {
            out.push_str(&ntop4(&[src[12], src[13], src[14], src[15]]));
            return out;
        }
        out.push_str(&format!("{:x}", words[i]));
        i += 1;
    }
    if best_base != -1 && best_base + best_len == 8 {
        out.push(':');
    }
    out
}

/// A Lean string's bytes, or `None` if it contains a NUL (C string conversion would truncate).
pub unsafe fn c_str_bytes<'a>(s: Obj) -> Option<&'a [u8]> {
    unsafe {
        let b = lean_string_bytes(s);
        if b.contains(&0) { None } else { Some(b) }
    }
}

// ---------------------------------------------------------------------------------------------
// Network interfaces
// ---------------------------------------------------------------------------------------------

pub struct Interface {
    pub name: String,
    pub mac: [u8; 6],
    pub internal: bool,
    pub address: IpAddr,
    pub netmask: IpAddr,
}

/// `uv_interface_addresses`: addresses of interfaces that are up and running.
#[cfg(unix)]
pub fn interface_addresses() -> std::io::Result<Vec<Interface>> {
    use std::ffi::CStr;
    unsafe {
        let mut addrs: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut addrs) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        #[cfg(any(target_os = "linux", target_os = "android"))]
        const LINK: i32 = libc::AF_PACKET;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        const LINK: i32 = libc::AF_LINK;
        let excluded = |ent: &libc::ifaddrs| -> bool {
            let flags = ent.ifa_flags as i32;
            !((flags & libc::IFF_UP) != 0 && (flags & libc::IFF_RUNNING) != 0) || ent.ifa_addr.is_null()
        };
        let mut out: Vec<Interface> = Vec::new();
        let mut ent = addrs;
        while !ent.is_null() {
            let e = &*ent;
            ent = e.ifa_next;
            if excluded(e) || (*e.ifa_addr).sa_family as i32 == LINK {
                continue;
            }
            let Some(address) = super::sys::sockaddr_from_raw(e.ifa_addr as *const _) else { continue };
            let netmask =
                if e.ifa_netmask.is_null() { None } else { super::sys::sockaddr_from_raw(e.ifa_netmask as *const _) };
            let netmask = match (netmask, address) {
                (Some(m), _) => m.ip(),
                (None, SocketAddr::V4(_)) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                (None, SocketAddr::V6(_)) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            };
            out.push(Interface {
                name: CStr::from_ptr(e.ifa_name).to_string_lossy().into_owned(),
                mac: [0; 6],
                internal: (e.ifa_flags as i32 & libc::IFF_LOOPBACK) != 0,
                address: address.ip(),
                netmask,
            });
        }
        // Physical addresses come from the link-layer entries of the same interface.
        let mut ent = addrs;
        while !ent.is_null() {
            let e = &*ent;
            ent = e.ifa_next;
            if excluded(e) || (*e.ifa_addr).sa_family as i32 != LINK {
                continue;
            }
            let name = CStr::from_ptr(e.ifa_name).to_string_lossy();
            let mut mac = [0u8; 6];
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                let ll = &*(e.ifa_addr as *const libc::sockaddr_ll);
                mac.copy_from_slice(&ll.sll_addr[..6]);
            }
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            {
                let dl = e.ifa_addr as *const libc::sockaddr_dl;
                let data = (std::ptr::addr_of!((*dl).sdl_data) as *const u8).add((*dl).sdl_nlen as usize);
                std::ptr::copy_nonoverlapping(data, mac.as_mut_ptr(), 6);
            }
            for iface in out.iter_mut().filter(|i| i.name == name) {
                iface.mac = mac;
            }
        }
        libc::freeifaddrs(addrs);
        Ok(out)
    }
}

/// `uv_interface_addresses`: unicast addresses of adapters that are up.
#[cfg(windows)]
pub fn interface_addresses() -> std::io::Result<Vec<Interface>> {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::NetworkManagement::IpHelper::*;
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;
    super::sys::init();
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 15 * 1024;
    let mut buf: Vec<u64>;
    loop {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        let r = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC as u32,
                flags,
                std::ptr::null(),
                buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        match r {
            ERROR_SUCCESS => break,
            ERROR_BUFFER_OVERFLOW => continue,
            ERROR_NO_DATA => return Ok(Vec::new()),
            e => return Err(std::io::Error::from_raw_os_error(e as i32)),
        }
    }
    let mut out = Vec::new();
    let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    unsafe {
        while !adapter.is_null() {
            let a = &*adapter;
            adapter = a.Next;
            if a.OperStatus != IfOperStatusUp || a.FirstUnicastAddress.is_null() {
                continue;
            }
            let mut len = 0;
            while *a.FriendlyName.add(len) != 0 {
                len += 1;
            }
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(a.FriendlyName, len));
            let mut mac = [0u8; 6];
            if a.PhysicalAddressLength as usize >= 6 {
                mac.copy_from_slice(&a.PhysicalAddress[..6]);
            }
            let internal = a.IfType == IF_TYPE_SOFTWARE_LOOPBACK;
            let mut unicast = a.FirstUnicastAddress;
            while !unicast.is_null() {
                let u = &*unicast;
                unicast = u.Next;
                let Some(address) = super::sys::sockaddr_from_raw(u.Address.lpSockaddr as *const _) else { continue };
                let prefix = u.OnLinkPrefixLength as u32;
                let netmask = match address {
                    SocketAddr::V4(_) => {
                        let bits = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix.min(32)) };
                        IpAddr::V4(Ipv4Addr::from(bits))
                    }
                    SocketAddr::V6(_) => {
                        let bits = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix.min(128)) };
                        IpAddr::V6(Ipv6Addr::from(bits))
                    }
                };
                out.push(Interface { name: name.clone(), mac, internal, address: address.ip(), netmask });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // TEST0233: Expected strings were obtained from Lean 4.34.1's `IPv4Addr.ofString`/`toString` and
    // `IPv6Addr.ofString`/`toString` (`lean --run`).
    #[test]
    fn test0233_ipv4_parsing_follows_inet_pton4() {
        assert_eq!(pton4(b"127.0.0.1"), Some([127, 0, 0, 1]));
        assert_eq!(pton4(b"255.255.255.255"), Some([255; 4]));
        assert_eq!(pton4(b"01.2.3.4"), None);
        assert_eq!(pton4(b"1.2.3"), None);
        assert_eq!(pton4(b"1.2.3.4.5"), None);
        assert_eq!(pton4(b"256.1.1.1"), None);
        assert_eq!(pton4(b"1..2.3"), None);
        assert_eq!(pton4(b" 1.2.3.4"), None);
    }

    /// TEST0234: ipv6 round trips like libuv
    #[test]
    fn test0234_ipv6_round_trips_like_libuv() {
        let cases = [
            ("::", "::"),
            ("::1", "::1"),
            ("1::", "1::"),
            ("2001:db8::ff00:42:8329", "2001:db8::ff00:42:8329"),
            ("2001:0db8:0000:0000:0000:ff00:0042:8329", "2001:db8::ff00:42:8329"),
            ("::ffff:192.168.1.1", "::ffff:192.168.1.1"),
            ("::192.168.1.1", "::192.168.1.1"),
            ("1:0:0:1:0:0:0:1", "1:0:0:1::1"),
            ("1:0:1:0:1:0:1:0", "1:0:1:0:1:0:1:0"),
            ("fe80::1%eth0", "fe80::1"),
            ("::0:1", "::1"),
            ("::0.0.0.1", "::1"),
            ("::ffff:0:0", "::ffff:0.0.0.0"),
            ("0:0:0:0:0:0:0:1", "::1"),
        ];
        for (input, expected) in cases {
            let parsed = uv_pton6(input.as_bytes()).unwrap_or_else(|| panic!("{input} should parse"));
            assert_eq!(ntop6(&parsed), expected, "formatting {input}");
        }
        for bad in [":1", "1:::2", "12345::", "1:2:3:4:5:6:7:8:9", "g::", "1::2::3", "1:"] {
            assert_eq!(uv_pton6(bad.as_bytes()), None, "{bad} should be rejected");
        }
    }
}
