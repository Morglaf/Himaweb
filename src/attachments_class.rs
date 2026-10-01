//! Classification des pièces jointes « accessoires » (signatures, clés, pixels, effets).

/// Une PJ est accessoire si elle n'est pas une pièce jointe utile pour l'utilisateur.
pub fn is_accessory_attachment(filename: &str, mime: &str, size: u64) -> bool {
    let mime_l = mime.trim().to_ascii_lowercase();
    let name = filename.trim();
    let name_l = name.to_ascii_lowercase();

    if is_pgp_or_signature_mime(&mime_l) {
        return true;
    }

    if name_l == "smime.p7s" || name_l == "smime.p7m" {
        return true;
    }

    // OpenPGP_signature…, OpenPGP_0xDEADBEEF.asc, etc.
    if name_l.starts_with("openpgp_") {
        return true;
    }

    if name_l.ends_with(".asc")
        && (is_pgp_or_signature_mime(&mime_l)
            || mime_l == "application/octet-stream"
            || mime_l == "text/plain"
            || mime_l.is_empty())
    {
        return true;
    }

    if size == 0 {
        return true;
    }

    if name_l.starts_with("lsi-attach-effects_") {
        return true;
    }

    if size < 2048 && is_small_graphic(&name_l, &mime_l) {
        return true;
    }

    false
}

fn is_pgp_or_signature_mime(mime: &str) -> bool {
    matches!(
        mime,
        "application/pgp-signature"
            | "application/pgp-keys"
            | "application/pgp-encrypted"
            | "application/pgp"
            | "application/pkcs7-signature"
            | "application/x-pkcs7-signature"
            | "application/pkcs7-mime"
            | "application/x-pkcs7-mime"
    ) || mime.starts_with("application/pgp-")
}

fn is_small_graphic(name_l: &str, mime_l: &str) -> bool {
    if mime_l.starts_with("image/") {
        return true;
    }
    if mime_l == "application/octet-stream" || mime_l.is_empty() {
        return name_l.ends_with(".png")
            || name_l.ends_with(".gif")
            || name_l.ends_with(".jpg")
            || name_l.ends_with(".jpeg")
            || name_l.ends_with(".webp")
            || name_l.ends_with(".svg");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgp_and_zero_byte() {
        assert!(is_accessory_attachment(
            "OpenPGP_signature.asc",
            "application/pgp-signature",
            677
        ));
        assert!(is_accessory_attachment(
            "lsi-attach-effects_abc.png",
            "application/octet-stream",
            0
        ));
    }

    #[test]
    fn pgp_keys_asc() {
        assert!(is_accessory_attachment(
            "OpenPGP_0xD733E541F93E3E0E.asc",
            "application/pgp-keys",
            3800
        ));
    }

    #[test]
    fn real_pdf_kept() {
        assert!(!is_accessory_attachment(
            "invoice.pdf",
            "application/pdf",
            12_000
        ));
    }

    #[test]
    fn tiny_png_octet_stream() {
        assert!(is_accessory_attachment(
            "pixel.png",
            "application/octet-stream",
            120
        ));
    }
}
