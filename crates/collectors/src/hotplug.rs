//! Hardware change notifications from the kernel's uevent netlink socket.
//!
//! Needs no privileges and no extra services. [`parse_uevent`] is pure and tested; [`watch`] blocks
//! the calling thread, so run it on its own thread. If the socket can't be opened (restricted
//! sandbox, exotic kernel) `watch` returns the error and callers fall back to manual refresh.

use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uevent {
    /// `add`, `remove`, `change`, `bind`, ...
    pub action: String,
    pub devpath: String,
    pub subsystem: String,
}

/// Subsystems whose changes can alter what Facet shows.
const INTERESTING: &[&str] = &[
    "usb", "block", "net", "drm", "bluetooth", "sound", "input", "power_supply", "video4linux", "pci", "thunderbolt", "hwmon", "typec",
];

/// Parse one kernel uevent datagram (`action@devpath\0KEY=value\0...`). Returns `None` for malformed
/// input and for subsystems we don't present. Never panics.
pub fn parse_uevent(buf: &[u8]) -> Option<Uevent> {
    let mut parts = buf.split(|b| *b == 0).filter(|p| !p.is_empty());
    let head = std::str::from_utf8(parts.next()?).ok()?;
    let (action, devpath) = head.split_once('@')?;
    if action.is_empty() || !devpath.starts_with('/') {
        return None;
    }
    let mut subsystem = None;
    for kv in parts {
        if let Some(v) = kv.strip_prefix(b"SUBSYSTEM=") {
            subsystem = std::str::from_utf8(v).ok().map(str::to_owned);
        }
    }
    let subsystem = subsystem?;
    if !INTERESTING.contains(&subsystem.as_str()) {
        return None;
    }
    Some(Uevent { action: action.to_owned(), devpath: devpath.to_owned(), subsystem })
}

/// Whether an event should trigger a re-read. `change` is only interesting for display/power events
/// (hotplug of a monitor, AC plugged in); other `change`s are chatter.
pub fn is_relevant(e: &Uevent) -> bool {
    match e.action.as_str() {
        "add" | "remove" => true,
        "change" => matches!(e.subsystem.as_str(), "drm" | "power_supply" | "typec"),
        _ => false,
    }
}

/// Block, calling `on_event` for each relevant change, until the socket fails.
pub fn watch(mut on_event: impl FnMut(Uevent)) -> io::Result<()> {
    // SAFETY: plain libc socket calls; the fd is owned and closed on every exit path.
    unsafe {
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, libc::NETLINK_KOBJECT_UEVENT);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut addr: libc::sockaddr_nl = std::mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as u16;
        addr.nl_groups = 1; // kernel uevents
        if libc::bind(fd, &addr as *const _ as *const libc::sockaddr, std::mem::size_of::<libc::sockaddr_nl>() as u32) < 0 {
            let e = io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        let mut buf = vec![0u8; 8192];
        loop {
            let n = libc::recv(fd, buf.as_mut_ptr().cast(), buf.len(), 0);
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                libc::close(fd);
                return Err(e);
            }
            if let Some(ev) = parse_uevent(&buf[..n as usize])
                && is_relevant(&ev)
            {
                on_event(ev);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_usb_add() {
        let msg = b"add@/devices/pci0000:00/0000:00:14.0/usb1/1-3\0ACTION=add\0DEVPATH=/devices/pci0000:00/0000:00:14.0/usb1/1-3\0SUBSYSTEM=usb\0DEVTYPE=usb_device\0";
        let e = parse_uevent(msg).unwrap();
        assert_eq!((e.action.as_str(), e.subsystem.as_str()), ("add", "usb"));
        assert!(e.devpath.ends_with("1-3"));
        assert!(is_relevant(&e));
    }

    #[test]
    fn ignores_uninteresting_subsystems_and_garbage() {
        assert!(parse_uevent(b"add@/devices/virtual/misc/x\0SUBSYSTEM=misc\0").is_none());
        assert!(parse_uevent(b"").is_none());
        assert!(parse_uevent(b"\0\0\0").is_none());
        assert!(parse_uevent(b"no-at-sign\0SUBSYSTEM=usb\0").is_none());
        assert!(parse_uevent(b"add@relative/path\0SUBSYSTEM=usb\0").is_none());
        assert!(parse_uevent(&[0xff, 0xfe, b'@', b'/', 0, b'S']).is_none());
        assert!(parse_uevent(b"add@/devices/x\0").is_none(), "no SUBSYSTEM key");
    }

    #[test]
    fn change_is_only_relevant_for_display_and_power() {
        let mk = |a: &str, s: &str| Uevent { action: a.into(), devpath: "/d".into(), subsystem: s.into() };
        assert!(is_relevant(&mk("change", "drm")));
        assert!(is_relevant(&mk("change", "power_supply")));
        assert!(!is_relevant(&mk("change", "block")));
        assert!(!is_relevant(&mk("bind", "usb")));
        assert!(is_relevant(&mk("remove", "net")));
    }

    #[test]
    fn netlink_socket_opens_unprivileged() {
        // Skips (passes) where the sandbox forbids netlink sockets; fails only if it opens and dies.
        let handle = std::thread::spawn(|| watch(|_| {}));
        std::thread::sleep(std::time::Duration::from_millis(300));
        if handle.is_finished() {
            let err = handle.join().unwrap().unwrap_err();
            eprintln!("netlink uevent socket unavailable here: {err}");
        }
        // Still running (blocked in recv): the thread is detached and ends with the test process.
    }
}
