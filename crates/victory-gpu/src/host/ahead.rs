//! Defaults for ahead-of-frame work on the adapter used by the D3D12 device.

use std::ffi::OsStr;
use std::sync::OnceLock;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    pub(crate) ahead_first: bool,
    pub(crate) card_flagged: bool,
    pub(crate) passes_ahead: bool,
}

fn is_rdna2(vendor: u32, device: u32) -> bool {
    // Dawn's AMD PCI architecture groups (mask 0xfff0), plus the exact 0x163f APU.
    // https://github.com/google/dawn/blob/55fe3b02cb96c6b74478d0624fb5e0ecede4e5e0/src/dawn/gpu_info.json
    vendor == 0x1002
        && (device == 0x163f
            || matches!(
                device & !0xf,
                0x13c0
                    | 0x13f0
                    | 0x1430
                    | 0x15e0
                    | 0x1640
                    | 0x1680
                    | 0x73a0
                    | 0x73b0
                    | 0x73d0
                    | 0x73e0
                    | 0x73f0
                    | 0x7400
                    | 0x7420
                    | 0x7430
            ))
}

fn resolve(
    rdna2: bool,
    thin: bool,
    ahead_first: Option<&OsStr>,
    card_flagged: Option<&OsStr>,
    passes_ahead: Option<&str>,
) -> Settings {
    Settings {
        ahead_first: ahead_first.map_or(!rdna2, |v| v != "0"),
        card_flagged: card_flagged.map_or(!rdna2, |v| v != "0"),
        passes_ahead: passes_ahead.map_or(!rdna2 && thin, |v| v.trim() != "0"),
    }
}

pub(crate) fn settings() -> &'static Settings {
    static SETTINGS: OnceLock<Settings> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let desc = super::host()
            .and_then(|h| h.adapter.as_ref())
            .and_then(|a| {
                // SAFETY: read-only DXGI descriptor for the device's own adapter.
                unsafe { a.GetDesc1() }.ok()
            });
        let rdna2 = desc.is_some_and(|d| is_rdna2(d.VendorId, d.DeviceId));
        let selected = resolve(
            rdna2,
            crate::list::thin::wanted(),
            std::env::var_os("VICTORY_AHEAD_FIRST").as_deref(),
            std::env::var_os("VICTORY_CARD_FLAGGED").as_deref(),
            std::env::var("VICTORY_PASSES_AHEAD").ok().as_deref(),
        );
        tracing::info!(
            rdna2,
            ahead_first = selected.ahead_first,
            card_flagged = selected.card_flagged,
            passes_ahead = selected.passes_ahead,
            "host submission settings"
        );
        selected
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdna2_matches_the_selected_vendor_and_device() {
        for id in [0x73bf, 0x73df, 0x73ff, 0x743f, 0x1681, 0x163f] {
            assert!(is_rdna2(0x1002, id));
            assert!(!is_rdna2(0x10de, id));
        }
        for id in [0x1636, 0x1638, 0x731f, 0x744c, 0x7550, 0xffff] {
            assert!(!is_rdna2(0x1002, id));
        }
        assert!(!is_rdna2(0x1414, 0x008c));
    }

    #[test]
    fn defaults_and_overrides_are_independent() {
        for thin in [false, true] {
            assert_eq!(
                resolve(true, thin, None, None, None),
                Settings {
                    ahead_first: false,
                    card_flagged: false,
                    passes_ahead: false
                }
            );
            assert_eq!(
                resolve(false, thin, None, None, None),
                Settings {
                    ahead_first: true,
                    card_flagged: true,
                    passes_ahead: thin
                }
            );
        }
        for rdna2 in [false, true] {
            for (first, flagged, passes) in [("1", "0", " 1 "), ("0", "1", " 0 ")] {
                assert_eq!(
                    resolve(
                        rdna2,
                        false,
                        Some(first.as_ref()),
                        Some(flagged.as_ref()),
                        Some(passes)
                    ),
                    Settings {
                        ahead_first: first == "1",
                        card_flagged: flagged == "1",
                        passes_ahead: passes.trim() == "1"
                    }
                );
            }
        }
    }
}
