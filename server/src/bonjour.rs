//! Advertises the party on the local network with Bonjour, as `_quizparty._tcp`, so that the iOS app can list it
//! without the player typing an address.
//!
//! It asks macOS's own mDNS responder, through the DNS Service Discovery API in the system library, rather than
//! running a responder of its own, which would compete with the system's for the mDNS port. The game code is not
//! advertised: a player still needs it from the big screen, so that only those in the room can join.

/// A registration with the mDNS responder, withdrawn when it is dropped.
pub struct Advert {
    /// The registration, kept for as long as the party is advertised.
    #[cfg(target_os = "macos")]
    service: ffi::DNSServiceRef,
}

// The registration is only an opaque handle, used again only to withdraw it, which the API allows from any thread.
unsafe impl Send for Advert {}
unsafe impl Sync for Advert {}

impl Advert {
    /// Advertises the party's web server on `port`, under the Mac's computer name. The TXT record gives the protocol
    /// version and the WebSocket's path.
    #[cfg(target_os = "macos")]
    pub fn new(port: u16) -> anyhow::Result<Advert> {
        use std::ffi::CString;

        let service_type = CString::new(protocol::BONJOUR_SERVICE)?;
        let txt = txt_record(&[
            ("protocol", &protocol::PROTOCOL_VERSION.to_string()),
            ("path", protocol::WEBSOCKET_PATH),
        ]);
        let mut service: ffi::DNSServiceRef = std::ptr::null_mut();
        // SAFETY: every pointer is valid for the call, the strings are NUL-terminated, and the TXT record's length is
        // its own. With no callback, the responder keeps the registration until the handle is deallocated.
        let error = unsafe {
            ffi::DNSServiceRegister(
                &mut service,
                0,
                0,
                std::ptr::null(),
                service_type.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                port.to_be(),
                txt.len() as u16,
                txt.as_ptr().cast(),
                None,
                std::ptr::null_mut(),
            )
        };
        anyhow::ensure!(error == 0, "the mDNS responder refused the registration, error {error}");
        Ok(Advert { service })
    }

    /// Bonjour is only used on macOS: elsewhere players join with the QR code or the address.
    #[cfg(not(target_os = "macos"))]
    pub fn new(_port: u16) -> anyhow::Result<Advert> {
        anyhow::bail!("Bonjour is only advertised on macOS")
    }
}

#[cfg(target_os = "macos")]
impl Drop for Advert {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful DNSServiceRegister and is deallocated only here.
        unsafe { ffi::DNSServiceRefDeallocate(self.service) }
    }
}

/// A DNS TXT record: each `key=value` string preceded by its length in a byte.
fn txt_record(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut record = Vec::new();
    for (key, value) in entries {
        let entry = format!("{key}={value}");
        let bytes = &entry.as_bytes()[..entry.len().min(255)];
        record.push(bytes.len() as u8);
        record.extend_from_slice(bytes);
    }
    record
}

/// The part of `<dns_sd.h>` used here, from the system library.
#[cfg(target_os = "macos")]
mod ffi {
    use std::ffi::{c_char, c_void};

    /// An opaque handle to a request to the mDNS responder.
    pub type DNSServiceRef = *mut c_void;

    /// Called when a registration completes or fails; not used, as no callback is passed.
    pub type DNSServiceRegisterReply =
        Option<unsafe extern "C" fn(DNSServiceRef, u32, i32, *const c_char, *const c_char, *const c_char, *mut c_void)>;

    unsafe extern "C" {
        /// Registers a service; `port` is in network byte order.
        #[allow(clippy::too_many_arguments)]
        pub fn DNSServiceRegister(
            service: *mut DNSServiceRef,
            flags: u32,
            interface_index: u32,
            name: *const c_char,
            regtype: *const c_char,
            domain: *const c_char,
            host: *const c_char,
            port: u16,
            txt_len: u16,
            txt_record: *const c_void,
            callback: DNSServiceRegisterReply,
            context: *mut c_void,
        ) -> i32;

        /// Withdraws a registration and frees its handle.
        pub fn DNSServiceRefDeallocate(service: DNSServiceRef);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn txt_records_are_length_prefixed() {
        assert_eq!(
            super::txt_record(&[("protocol", "1"), ("path", "/ws")]),
            b"\x0aprotocol=1\x08path=/ws"
        );
    }
}
