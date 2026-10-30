// Derived from Scid vs PC 4.27, src/tkscid.cpp (`sc_game_tags_share`,
// 10098-10320, "check" mode). Copyright (c) 1999-2004 Shane Hudson; Scid vs PC
// contributors. Scid is distributed under the GNU General Public License (see
// its COPYING); this file is part of MyChess, GPL-3.0-or-later
// (docs/adr/0004-licenca.md).

//! The "check" half of `sc_game tags share`: the list the twin checker shows
//! before the user agrees (decision D3514). The write is
//! [`Si4Writer::share_tags`](crate::si4::Si4Writer::share_tags).

use crate::si4::{SharedTag, plan_share_tags};
use crate::source::GameSource;

/// What sharing the tags of games `a` and `b` would change, without writing.
/// Equal numbers, `0` or numbers outside the database give an empty list
/// (rule 26); so does an entry that cannot be read.
pub fn share_tags_preview<D: GameSource + ?Sized>(db: &D, a: u32, b: u32) -> Vec<SharedTag> {
    let len = db.len();
    if a == 0 || b == 0 || a == b || a > len || b > len {
        return Vec::new();
    }
    let (Ok(entry_a), Ok(entry_b)) = (db.entry(a), db.entry(b)) else {
        return Vec::new();
    };
    let names = db.names();
    plan_share_tags(a, &entry_a, b, &entry_b, &|kind, id| {
        names.get(kind, id).map(<[u8]>::to_vec)
    })
}
