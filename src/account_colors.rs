//! Utilitaires partagés pour couleurs de comptes HimaWeb.

pub const ACCOUNT_PALETTE: &[&str] = &[
    "#2563eb", // blue
    "#dc2626", // red
    "#059669", // green
    "#d97706", // amber
    "#7c3aed", // violet
    "#db2777", // pink
    "#0891b2", // cyan
    "#65a30d", // lime
    "#ea580c", // orange
    "#4f46e5", // indigo
];

/// Icônes Lucide de secours (le picker charge le catalogue complet via CDN).
#[allow(dead_code)]
pub const ACCOUNT_ICON_CHOICES: &[&str] = &[
    "circle-user",
    "mail",
    "briefcase",
    "home",
    "building-2",
    "graduation-cap",
    "laptop",
    "smartphone",
    "globe",
    "heart",
    "star",
    "zap",
    "coffee",
    "bookmark",
    "shield",
    "users",
];
pub fn default_color_for(name: &str) -> String {
    let mut h: u32 = 0;
    for b in name.as_bytes() {
        h = h.wrapping_mul(31).wrapping_add(u32::from(*b));
    }
    ACCOUNT_PALETTE[(h as usize) % ACCOUNT_PALETTE.len()].to_string()
}
