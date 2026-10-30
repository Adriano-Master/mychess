//! The translated words of the opening report window: the remarks of the
//! application (spec M7-2c).

use fluent_bundle::FluentArgs;
use mychess_app::opening_report::OpeningReportNotice;

use crate::i18n::I18n;

/// The text of a remark about the opening report. The two notices with
/// numbers get them as arguments (D3657).
pub fn notice_text(i18n: &I18n, notice: &OpeningReportNotice) -> String {
    let mut args = FluentArgs::new();
    match notice {
        OpeningReportNotice::FavoritesImported { added, skipped } => {
            args.set("added", i64::from(*added));
            args.set("skipped", i64::from(*skipped));
        }
        OpeningReportNotice::BatchDone { written, failed } => {
            args.set("written", i64::from(*written));
            args.set("failed", i64::from(*failed));
        }
        _ => {}
    }
    i18n.tr_args(notice.message_key(), &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn the_numbers_of_the_two_notices_reach_the_text() {
        let en = I18n::new(Language::EnUs);
        let text = notice_text(
            &en,
            &OpeningReportNotice::FavoritesImported {
                added: 3,
                skipped: 2,
            },
        );
        assert!(text.contains('3') && text.contains('2'), "{text}");
        let pt = I18n::new(Language::PtBr);
        let text = notice_text(
            &pt,
            &OpeningReportNotice::BatchDone {
                written: 7,
                failed: 1,
            },
        );
        assert!(text.contains('7') && text.contains('1'), "{text}");
    }
}
