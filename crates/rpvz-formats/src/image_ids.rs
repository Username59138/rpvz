//! Mapping from the numeric image ids inside compiled reanims to image
//! resource ids such as `IMAGE_REANIM_PEASHOOTER_HEAD`.
//!
//! Compiled reanims carry the original game's image numbering, which neither
//! the RSB nor `libpvz.so` spells out. It was recovered by aligning every id
//! used in the 119 reanims with the image list below, using track and file
//! names as evidence: all 1180 used ids match. The result is a handful of
//! ranges that are each a constant shift of that list.
//!
//! Only numbers live here; the names come from the user's own `main.rsb`.

use crate::rsb::{Group, ImageKind, ResourceKind};

/// `(first_id, last_id, shift)`: id `n` in `first..=last` is entry `n + shift`
/// of [`ImageIdTable`]'s list.
const SEGMENTS: &[(u32, u32, i32)] = &[
    (190, 197, -86),
    (261, 748, 172),
    (749, 774, 366),
    (776, 826, 172),
    (828, 848, 326),
    (850, 864, 324),
    (866, 921, 172),
    (922, 931, 977),
    (932, 1747, 172),
    (1748, 1758, -499),
    (1759, 1767, 172),
    (1768, 1781, -519),
    (1782, 1912, 172),
    (1949, 1996, -1697),
    (2004, 2062, -1818),
    (2100, 2106, -1758),
    (2127, 2138, -1756),
    (2139, 2240, -44),
    (2337, 2341, -1927),
    (2354, 2402, -60),
    (2403, 2452, -98),
];

/// Locale whose translated images (e.g. "Final Wave") take part in the numbering.
const NUMBERING_LOCALE: &str = "ENUS";

#[derive(Debug, Clone)]
pub struct ImageIdTable {
    names: Vec<String>,
}

impl ImageIdTable {
    /// Builds the list from an RSB description: unique base ids of non-atlas
    /// images, in description order, taking 480 or resolution-independent
    /// subgroups that are shared or English.
    pub fn from_description(groups: &[Group]) -> Self {
        let mut names = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sub in groups.iter().flat_map(|g| &g.subgroups) {
            if !matches!(sub.resolution, 0 | 480) {
                continue;
            }
            if sub.locale.as_deref().is_some_and(|l| l != NUMBERING_LOCALE) {
                continue;
            }
            for res in &sub.resources {
                let is_atlas = res
                    .image
                    .as_ref()
                    .is_some_and(|i| i.kind == ImageKind::Atlas);
                if res.kind != ResourceKind::Image || is_atlas {
                    continue;
                }
                let base = res.base_id();
                if seen.insert(base.to_owned()) {
                    names.push(base.to_owned());
                }
            }
        }
        Self { names }
    }

    /// Resource id for a reanim image id, if the id is one the game uses.
    pub fn name(&self, id: u32) -> Option<&str> {
        let &(_, _, shift) = SEGMENTS
            .iter()
            .find(|(lo, hi, _)| (*lo..=*hi).contains(&id))?;
        let index = usize::try_from(i64::from(id) + i64::from(shift)).ok()?;
        self.names.get(index).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_sorted_and_disjoint() {
        for w in SEGMENTS.windows(2) {
            assert!(w[0].0 <= w[0].1 && w[0].1 < w[1].0, "{:?}", w);
        }
    }

    #[test]
    fn unknown_ids_resolve_to_none() {
        let t = ImageIdTable {
            names: (0..3000).map(|i| format!("IMAGE_{i}")).collect(),
        };
        assert_eq!(t.name(0), None);
        assert_eq!(t.name(1920), None);
        assert_eq!(t.name(261), Some("IMAGE_433"));
    }
}
