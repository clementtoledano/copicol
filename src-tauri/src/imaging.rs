//! Traitement des images du presse-papiers : décodage, PNG, miniature, hash
//! des pixels, limites, et choix de la source à capturer (texte ou image).

use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::Path;

/// Au-delà de ce nombre de pixels (50 Mpx), l'image est ignorée.
pub const MAX_PIXELS: u64 = 50_000_000;
/// Au-delà de cette taille de PNG (20 Mo), l'image est ignorée.
pub const MAX_PNG_BYTES: usize = 20 * 1024 * 1024;
/// Dimensions maximales de la miniature de la liste.
pub const THUMB_MAX: (u32, u32) = (200, 150);
/// Extensions des fichiers image capturés depuis l'Explorateur.
pub const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];

/// Image décodée : pixels RGBA 8 bits, `pixels.len() == width * height * 4`.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Hash de dédoublonnage d'une image, calculé sur les pixels décodés. Le
/// préfixe `img:` évite toute collision avec le hash d'un texte.
pub fn image_hash(img: &Rgba) -> String {
    let mut hasher = Sha256::new();
    hasher.update(img.width.to_le_bytes());
    hasher.update(img.height.to_le_bytes());
    hasher.update(&img.pixels);
    format!("img:{:x}", hasher.finalize())
}

pub fn within_limits(width: u32, height: u32) -> bool {
    width > 0 && height > 0 && (width as u64) * (height as u64) <= MAX_PIXELS
}

/// Décode un format reconnu (PNG, JPEG, GIF — première image —, BMP, WebP).
pub fn decode(bytes: &[u8]) -> Result<Rgba, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?.to_rgba8();
    Ok(Rgba { width: img.width(), height: img.height(), pixels: img.into_raw() })
}

pub fn encode_png(img: &Rgba) -> Result<Vec<u8>, String> {
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())
        .ok_or_else(|| "dimensions incohérentes avec les pixels".to_string())?;
    let mut out = Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// Miniature PNG tenant dans [`THUMB_MAX`], ratio conservé, jamais agrandie.
pub fn thumbnail_png(img: &Rgba) -> Result<Vec<u8>, String> {
    let scale = (THUMB_MAX.0 as f64 / img.width as f64)
        .min(THUMB_MAX.1 as f64 / img.height as f64)
        .min(1.0);
    let w = ((img.width as f64 * scale).round() as u32).max(1);
    let h = ((img.height as f64 * scale).round() as u32).max(1);
    let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone())
        .ok_or_else(|| "dimensions incohérentes avec les pixels".to_string())?;
    let small = image::imageops::resize(&buf, w, h, image::imageops::FilterType::Triangle);
    encode_png(&Rgba { width: w, height: h, pixels: small.into_raw() })
}

pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Vrai si le HTML du presse-papiers se réduit à une unique balise `<img>`
/// (« Copier l'image » d'un navigateur), sans texte autour.
pub fn html_is_single_img(html: &str) -> bool {
    use regex::Regex;
    let fragment = match (html.find("<!--StartFragment-->"), html.find("<!--EndFragment-->")) {
        (Some(a), Some(b)) if a < b => &html[a + "<!--StartFragment-->".len()..b],
        _ => html,
    };
    let comments = Regex::new(r"(?s)<!--.*?-->").unwrap();
    let wrappers = Regex::new(r"(?is)</?(html|head|body|meta|span)\b[^>]*>").unwrap();
    let cleaned = comments.replace_all(fragment, "");
    let cleaned = wrappers.replace_all(&cleaned, "");
    let single = Regex::new(r"(?is)^\s*<img\b[^>]*>\s*$").unwrap();
    single.is_match(&cleaned)
}

#[derive(Debug, PartialEq)]
pub enum CaptureSource {
    Text,
    Image,
    Nothing,
}

/// Choisit ce qu'on capture : le texte d'abord (Excel et Word posent aussi un
/// bitmap), sauf si le HTML se réduit à une image (« Copier l'image » d'un
/// navigateur, qui ajoute l'URL en texte). `image_done` : l'image de cette
/// copie a déjà été traitée au poll précédent ; son URL en texte est alors
/// ignorée au lieu de devenir une entrée texte.
pub fn capture_source(
    text: Option<&str>,
    html: Option<&str>,
    has_image: bool,
    image_done: bool,
) -> CaptureSource {
    let has_text = text.map(|t| !t.trim().is_empty()).unwrap_or(false);
    if has_text {
        if html.map(html_is_single_img).unwrap_or(false) {
            if has_image {
                return CaptureSource::Image;
            }
            if image_done {
                return CaptureSource::Nothing;
            }
        }
        return CaptureSource::Text;
    }
    if has_image {
        CaptureSource::Image
    } else {
        CaptureSource::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(w: u32, h: u32) -> Rgba {
        Rgba { width: w, height: h, pixels: (0..w * h * 4).map(|i| (i % 251) as u8).collect() }
    }

    #[test]
    fn png_roundtrip_preserves_pixels() {
        let img = sample(37, 21);
        let back = decode(&encode_png(&img).unwrap()).unwrap();
        assert_eq!((back.width, back.height), (37, 21));
        assert_eq!(back.pixels, img.pixels);
        assert_eq!(image_hash(&back), image_hash(&img));
    }

    #[test]
    fn hash_is_prefixed_and_depends_on_dimensions() {
        let a = Rgba { width: 2, height: 1, pixels: vec![0; 8] };
        let b = Rgba { width: 1, height: 2, pixels: vec![0; 8] };
        assert!(image_hash(&a).starts_with("img:"));
        assert_ne!(image_hash(&a), image_hash(&b));
    }

    #[test]
    fn thumbnail_fits_and_keeps_ratio() {
        let t = decode(&thumbnail_png(&sample(1920, 1080)).unwrap()).unwrap();
        assert_eq!((t.width, t.height), (200, 113)); // 1080*200/1920 = 112,5 → arrondi
        let small = decode(&thumbnail_png(&sample(50, 40)).unwrap()).unwrap();
        assert_eq!((small.width, small.height), (50, 40)); // jamais agrandie
    }

    #[test]
    fn limits() {
        assert!(within_limits(7680, 4320)); // 8K = 33 Mpx
        assert!(!within_limits(10_000, 6_000)); // 60 Mpx
        assert!(!within_limits(0, 10));
    }

    #[test]
    fn decode_garbage_is_err() {
        assert!(decode(b"pas une image").is_err());
        let png = encode_png(&sample(10, 10)).unwrap();
        assert!(decode(&png[..png.len() / 2]).is_err()); // PNG tronqué
    }

    #[test]
    fn image_path_detection() {
        assert!(is_image_path(std::path::Path::new(r"C:\Users\Clément\Capture d'écran.PNG")));
        assert!(is_image_path(std::path::Path::new("photo.jpeg")));
        assert!(!is_image_path(std::path::Path::new("notes.txt")));
        assert!(!is_image_path(std::path::Path::new("sans_extension")));
    }

    #[test]
    fn single_img_html() {
        assert!(html_is_single_img(
            "<html><body><!--StartFragment--><img src=\"https://ex.com/a.png\" alt=\"x\"><!--EndFragment--></body></html>"
        ));
        assert!(!html_is_single_img("<table><tr><td>1</td></tr></table>"));
        assert!(!html_is_single_img("<p>texte <img src=\"a.png\"></p>")); // texte autour
        assert!(!html_is_single_img("<img src=a.png><img src=b.png>"));
    }

    #[test]
    fn capture_source_excel_keeps_text() {
        let html = "<table><tr><td>2610S0200C005</td></tr></table>";
        assert_eq!(capture_source(Some("2610S0200C005"), Some(html), true, false), CaptureSource::Text);
    }

    #[test]
    fn capture_source_browser_image_wins_over_url() {
        let html = "<img src=\"https://ex.com/a.png\">";
        assert_eq!(capture_source(Some("https://ex.com/a.png"), Some(html), true, false), CaptureSource::Image);
    }

    #[test]
    fn capture_source_basic_cases() {
        assert_eq!(capture_source(None, None, true, false), CaptureSource::Image); // capture d'écran
        assert_eq!(capture_source(Some("bonjour"), None, false, false), CaptureSource::Text);
        assert_eq!(capture_source(Some("   "), None, false, false), CaptureSource::Nothing);
        assert_eq!(capture_source(Some("  "), None, true, false), CaptureSource::Image);
        assert_eq!(capture_source(None, None, false, false), CaptureSource::Nothing);
    }

    #[test]
    fn capture_source_browser_url_after_image_done_is_skipped() {
        // Au poll suivant la capture, l'image n'est plus relue : l'URL en texte
        // du même « Copier l'image » ne doit pas devenir une entrée texte.
        let html = "<img src=\"https://ex.com/a.png\">";
        assert_eq!(
            capture_source(Some("https://ex.com/a.png"), Some(html), false, true),
            CaptureSource::Nothing
        );
        // Un vrai texte (sans <img> seul) reste capturé même si l'image est traitée
        assert_eq!(capture_source(Some("bonjour"), Some("<p>bonjour</p>"), false, true), CaptureSource::Text);
    }
}
