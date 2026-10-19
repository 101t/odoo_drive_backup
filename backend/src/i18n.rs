//! Language of the few texts the server composes for the UI (cron descriptions). Everything else
//! the UI shows is translated in the browser.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderMap, header};
use croner::describe::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ar,
}

impl Lang {
    /// The UI sends its language in `Accept-Language`.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let wanted = headers.get(header::ACCEPT_LANGUAGE).and_then(|v| v.to_str().ok()).unwrap_or("");
        let first = wanted.split(',').next().unwrap_or("").trim().to_ascii_lowercase();
        if first == "ar" || first.starts_with("ar-") { Lang::Ar } else { Lang::En }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Lang {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Lang::from_headers(&parts.headers))
    }
}

/// Arabic wording for cron descriptions (see `croner::describe::Language`).
#[derive(Default, Clone, Copy)]
pub struct Arabic;

impl Arabic {
    /// croner assembles phrases with English punctuation and a few fixed English words in rare
    /// patterns; adapt those to Arabic.
    pub fn polish(description: &str) -> String {
        let mut text = description.replace(", ", "، ").replace(" past ", " خلال ").replace("hour ", "الساعة ");
        if let Some(start) = text.find("every ") {
            // "every N months" / "year every N"
            let rest = &text[start + 6..];
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            let after = &rest[digits.len()..];
            let (unit, consumed) = match after.strip_prefix(" months") {
                Some(_) => (" أشهر", " months".len()),
                None => ("", 0),
            };
            text = format!("{}كل {digits}{unit}{}", &text[..start], &after[consumed..]);
        }
        // "و" is written attached to the next word.
        text.replace(" و ", " و")
    }
}

impl Language for Arabic {
    fn every_minute(&self) -> &'static str {
        "كل دقيقة"
    }
    fn every_second_phrase(&self) -> &'static str {
        "كل ثانية"
    }
    fn every_x_minutes(&self, step: u16) -> String {
        format!("كل {step} دقيقة")
    }
    fn every_x_seconds(&self, step: u16) -> String {
        format!("كل {step} ثانية")
    }
    fn every_x_hours(&self, step: u16) -> String {
        format!("كل {step} ساعات")
    }
    fn every_minute_of_every_x_hours(&self, step: u16) -> String {
        format!("كل دقيقة، كل {step} ساعات")
    }

    fn at_time(&self, time: &str) -> String {
        format!("عند الساعة {time}")
    }
    fn at_time_and_every_x_seconds(&self, time: &str, step: u16) -> String {
        format!("عند الساعة {time}، كل {step} ثانية")
    }
    fn at_time_at_second(&self, time: &str, second: &str) -> String {
        format!("عند الساعة {time}، في الثانية {second}")
    }

    fn at_phrase(&self, phrase: &str) -> String {
        if phrase.starts_with("كل ") { phrase.to_string() } else { format!("عند {phrase}") }
    }
    fn on_phrase(&self, phrase: &str) -> String {
        format!("في {phrase}")
    }
    fn in_phrase(&self, phrase: &str) -> String {
        format!("في {phrase}")
    }

    fn second_phrase(&self, s: &str) -> String {
        format!("الثانية {s}")
    }
    fn minute_phrase(&self, s: &str) -> String {
        format!("الدقيقة {s}")
    }
    fn minute_past_every_hour_phrase(&self, s: &str) -> String {
        format!("{s} من كل ساعة")
    }
    fn hour_phrase(&self, s: &str) -> String {
        format!("من الساعة {s}")
    }
    fn year_phrase(&self, s: &str) -> String {
        format!("سنة {s}")
    }

    fn day_phrase(&self, s: &str) -> String {
        format!("اليوم {s} من الشهر")
    }
    fn the_last_day_of_the_month(&self) -> &'static str {
        "آخر يوم من الشهر"
    }
    fn the_weekday_nearest_day(&self, day: &str) -> String {
        format!("يوم العمل الأقرب إلى اليوم {day}")
    }
    fn the_last_weekday_of_the_month(&self, day: &str) -> String {
        format!("آخر {day} من الشهر")
    }
    fn the_nth_weekday_of_the_month(&self, n: u8, day: &str) -> String {
        let nth = match n {
            1 => "أول",
            2 => "ثاني",
            3 => "ثالث",
            4 => "رابع",
            _ => "خامس",
        };
        format!("{nth} {day} من الشهر")
    }

    fn dom_and_dow_if_also(&self, dow: &str) -> String {
        format!("(إذا كان أيضًا {dow})")
    }
    fn dom_and_dow_if_also_one_of(&self, dow: &str) -> String {
        format!("(إذا كان أيضًا أحد: {dow})")
    }

    fn list_conjunction_and(&self) -> &'static str {
        "و"
    }
    fn list_conjunction_or(&self) -> &'static str {
        "أو"
    }
    fn list_conjunction_and_comma(&self) -> &'static str {
        "، و"
    }

    fn day_of_week_names(&self) -> [&'static str; 7] {
        ["يوم الأحد", "يوم الاثنين", "يوم الثلاثاء", "يوم الأربعاء", "يوم الخميس", "يوم الجمعة", "يوم السبت"]
    }
    fn month_names(&self) -> [&'static str; 12] {
        ["يناير", "فبراير", "مارس", "أبريل", "مايو", "يونيو", "يوليو", "أغسطس", "سبتمبر", "أكتوبر", "نوفمبر", "ديسمبر"]
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::scheduler::describe;

    #[test]
    fn language_from_header() {
        let mut headers = HeaderMap::new();
        assert_eq!(Lang::from_headers(&headers), Lang::En);
        headers.insert(header::ACCEPT_LANGUAGE, HeaderValue::from_static("ar-LB,ar;q=0.9,en;q=0.8"));
        assert_eq!(Lang::from_headers(&headers), Lang::Ar);
        headers.insert(header::ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,ar;q=0.5"));
        assert_eq!(Lang::from_headers(&headers), Lang::En);
    }

    #[test]
    fn arabic_descriptions() {
        assert_eq!(describe("0 2 * * *", Lang::Ar), "عند الساعة 02:00.");
        assert_eq!(describe("30 1 * * 1,5", Lang::Ar), "عند الساعة 01:30، في يوم الاثنين ويوم الجمعة.");
        assert_eq!(describe("0 3 1 * *", Lang::Ar), "عند الساعة 03:00، في اليوم 1 من الشهر.");
        assert_eq!(describe("*/15 * * * *", Lang::Ar), "كل 15 دقيقة.");
        assert_eq!(describe("0 */6 * * *", Lang::Ar), "عند الدقيقة 0، كل 6 ساعات.");
        assert_eq!(describe("0 4 1 */3 *", Lang::Ar), "عند الساعة 04:00، في اليوم 1 من الشهر، في كل 3 أشهر.");
        assert_eq!(describe("0 2 * * *", Lang::En), "At 02:00.");
        for expression in ["* 0 * * *", "5 * * * *", "0 2 L * *", "0 2 * * 5#2", "0 2 * * 5L", "0 0,12 * * *"] {
            let text = describe(expression, Lang::Ar);
            assert!(!text.chars().any(|c| c.is_ascii_alphabetic()), "{expression}: {text}");
        }
    }
}
