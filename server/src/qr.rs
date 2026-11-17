//! QR codes, drawn as SVG for the webview, e.g. the join code on the big screen.

/// A QR code for some text, as an SVG which fills its container, dark on light so that every phone's camera reads it.
pub fn svg(text: &str) -> Option<String> {
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::svg::Color>()
            .quiet_zone(true)
            .dark_color(qrcode::render::svg::Color("#0f1116"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn draws_an_svg() {
        let svg = super::svg("http://192.168.1.2:8765/?code=ABCD").unwrap();
        assert!(svg.contains("<svg") && svg.contains("#0f1116"));
    }
}
