//! The wording on receipts that the user can change in Settings: the "How
//! payments work" heading and its lines (one list per loan type), the
//! captions under the two signature lines, and the closing line of each
//! receipt. Anything not changed prints the built-in wording in the current
//! language, which is exactly the text receipts always had.
//!
//! Lines carry amounts from the loan through tags such as `{interest}`. A
//! line that only applies to some loans (a late fee, a converted rate, an
//! edited schedule) keeps that rule when edited; lines the user adds always
//! print. Wording never changes a loan's terms or numbers.

use crate::domain::loans::Method;
use crate::i18n::t;

/// The loan types whose receipts explain payments differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleSet {
    Declining,
    Flat,
    InterestOnly,
}

impl RuleSet {
    pub const ALL: [RuleSet; 3] = [RuleSet::Declining, RuleSet::Flat, RuleSet::InterestOnly];

    pub fn of(method: Method) -> Self {
        match method {
            Method::Declining => RuleSet::Declining,
            Method::Flat => RuleSet::Flat,
            Method::InterestOnly => RuleSet::InterestOnly,
        }
    }
    pub fn method(self) -> Method {
        match self {
            RuleSet::Declining => Method::Declining,
            RuleSet::Flat => Method::Flat,
            RuleSet::InterestOnly => Method::InterestOnly,
        }
    }
    fn key(self) -> &'static str {
        match self {
            RuleSet::Declining => "receipt_rules_declining",
            RuleSet::Flat => "receipt_rules_flat",
            RuleSet::InterestOnly => "receipt_rules_interest_only",
        }
    }
    fn index(self) -> usize {
        self as usize
    }
}

/// When a line prints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Always,
    /// The rate is quoted for another period than the payments.
    Converted,
    /// The loan has a late fee.
    LateFee,
    /// The schedule was set by hand.
    ScheduleEdited,
}

impl When {
    fn key(self) -> &'static str {
        match self {
            When::Always => "always",
            When::Converted => "converted",
            When::LateFee => "late_fee",
            When::ScheduleEdited => "edited",
        }
    }
    fn from_key(k: &str) -> Option<Self> {
        [When::Always, When::Converted, When::LateFee, When::ScheduleEdited].into_iter().find(|w| w.key() == k)
    }
    /// Shown next to a line that doesn't always print.
    pub fn hint(self) -> Option<&'static str> {
        match self {
            When::Always => None,
            When::Converted => Some(t("Only when the rate is converted", "Solo si la tasa se convierte")),
            When::LateFee => Some(t("Only when the loan has a late fee", "Solo si el préstamo tiene mora")),
            When::ScheduleEdited => Some(t("Only when the schedule was set by hand", "Solo si el calendario se fijó a mano")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleLine {
    pub text: String,
    pub when: When,
}

impl RuleLine {
    fn new(when: When, text: impl Into<String>) -> Self {
        Self { text: text.into(), when }
    }
}

/// Tags a line can hold, with what each stands for.
pub fn tags() -> [(&'static str, &'static str); 8] {
    [
        ("{amount}", t("amount lent", "monto prestado")),
        ("{interest}", t("interest per payment", "interés por pago")),
        ("{late_fee}", t("late fee rate", "tasa de mora")),
        ("{conversion}", t("how the rate converts", "cómo se convierte la tasa")),
        ("{example_payment}", t("example payment (interest only)", "pago de ejemplo (solo interés)")),
        ("{example_capital}", t("its capital part", "su parte de capital")),
        ("{next_interest}", t("the interest after it", "el interés después")),
        ("{company}", t("company name", "nombre de la empresa")),
    ]
}

/// The built-in lines for a loan type, in the current language.
pub fn default_rules(set: RuleSet) -> Vec<RuleLine> {
    let conversion = RuleLine::new(
        When::Converted,
        t("The rate is converted on a 30-day month: {conversion}.", "La tasa se convierte sobre un mes de 30 días: {conversion}."),
    );
    let mut lines = vec![RuleLine::new(When::Always, set.method().explanation()), conversion];
    match set {
        RuleSet::Declining | RuleSet::Flat => lines.extend([
            RuleLine::new(
                When::LateFee,
                t(
                    "An installment not fully paid by its due date is charged a late fee of {late_fee} of what is still unpaid, once, the day after.",
                    "Una cuota que no se pague completa a su vencimiento tiene un cargo por mora de {late_fee} de lo que falte, una sola vez, al día siguiente.",
                ),
            ),
            RuleLine::new(
                When::Always,
                t(
                    "Payments cover late fees first, then interest, then capital. The loan can be paid off early with the capital owed plus the interest due.",
                    "Los pagos cubren primero los cargos por mora, luego el interés y después el capital. El préstamo se puede saldar antes con el capital adeudado más el interés pendiente.",
                ),
            ),
            RuleLine::new(When::ScheduleEdited, t("This schedule was set by agreement.", "Este calendario se fijó de común acuerdo.")),
        ]),
        RuleSet::InterestOnly => lines.extend([
            RuleLine::new(
                When::Always,
                t(
                    "Paying {interest} each time covers the interest; the capital stays at {amount}.",
                    "Si paga {interest} cada vez, cubre el interés y el capital sigue en {amount}.",
                ),
            ),
            RuleLine::new(
                When::Always,
                t(
                    "Anything paid above the interest lowers the capital. For example, paying {example_payment} is {interest} of interest and {example_capital} of capital, and the next interest drops to {next_interest}.",
                    "Todo lo que pague por encima del interés baja el capital. Por ejemplo, si paga {example_payment}, son {interest} de interés y {example_capital} de capital, y el próximo interés baja a {next_interest}.",
                ),
            ),
            RuleLine::new(
                When::Always,
                t(
                    "The loan can be paid off at any time with the capital owed plus the interest due. It closes when the capital reaches zero.",
                    "El préstamo se puede saldar en cualquier momento con el capital adeudado más el interés pendiente. Se cierra cuando el capital llega a cero.",
                ),
            ),
        ]),
    }
    lines
}

pub fn default_heading() -> String {
    t("How payments work", "Cómo funcionan los pagos").into()
}
/// Under the client's signature line; the client's name follows when
/// Settings prints it.
pub fn default_client_sign() -> String {
    t("Client", "Cliente").into()
}
pub fn default_lender_sign() -> String {
    t("For {company}", "Por {company}").into()
}
pub fn default_payment_footer() -> String {
    t("Thank you for your payment.", "Gracias por su pago.").into()
}
/// Loan receipts had no closing line.
pub fn default_loan_footer() -> String {
    String::new()
}

/// Receipt wording. `None` everywhere is the built-in text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReceiptText {
    pub heading: Option<String>,
    pub rules: [Option<Vec<RuleLine>>; 3],
    pub client_sign: Option<String>,
    pub lender_sign: Option<String>,
    pub payment_footer: Option<String>,
    pub loan_footer: Option<String>,
}

/// Stored values start with this; anything else (missing or empty) means
/// "built-in text", so an intentionally empty custom line can be stored.
const CUSTOM: char = '=';

fn encode(v: &Option<String>) -> String {
    v.as_ref().map(|s| format!("{CUSTOM}{}", one_line(s))).unwrap_or_default()
}
fn decode(v: Option<&str>) -> Option<String> {
    v.and_then(|s| s.strip_prefix(CUSTOM)).map(str::to_string)
}

/// Line breaks and tabs become spaces: every value is one line.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

impl ReceiptText {
    pub fn is_default(&self) -> bool {
        *self == ReceiptText::default()
    }

    pub fn heading(&self) -> String {
        self.heading.clone().unwrap_or_else(default_heading)
    }
    pub fn rules(&self, set: RuleSet) -> Vec<RuleLine> {
        self.rules[set.index()].clone().unwrap_or_else(|| default_rules(set))
    }
    pub fn set_rules(&mut self, set: RuleSet, lines: Option<Vec<RuleLine>>) {
        self.rules[set.index()] = lines;
    }
    pub fn client_sign(&self) -> String {
        self.client_sign.clone().unwrap_or_else(default_client_sign)
    }
    pub fn lender_sign(&self) -> String {
        self.lender_sign.clone().unwrap_or_else(default_lender_sign)
    }
    pub fn payment_footer(&self) -> String {
        self.payment_footer.clone().unwrap_or_else(default_payment_footer)
    }
    pub fn loan_footer(&self) -> String {
        self.loan_footer.clone().unwrap_or_else(default_loan_footer)
    }

    pub fn from_map(get: impl Fn(&str) -> Option<String>) -> Self {
        let mut rules: [Option<Vec<RuleLine>>; 3] = Default::default();
        for set in RuleSet::ALL {
            rules[set.index()] = decode(get(set.key()).as_deref()).map(|s| {
                s.split('\n')
                    .filter(|l| !l.is_empty())
                    .map(|l| match l.split_once('\t').and_then(|(w, text)| Some((When::from_key(w)?, text))) {
                        Some((when, text)) => RuleLine::new(when, text),
                        None => RuleLine::new(When::Always, l),
                    })
                    .collect()
            });
        }
        Self {
            heading: decode(get("receipt_rules_heading").as_deref()),
            rules,
            client_sign: decode(get("receipt_client_sign").as_deref()),
            lender_sign: decode(get("receipt_lender_sign").as_deref()),
            payment_footer: decode(get("receipt_payment_footer").as_deref()),
            loan_footer: decode(get("receipt_loan_footer").as_deref()),
        }
    }

    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            ("receipt_rules_heading", encode(&self.heading)),
            ("receipt_client_sign", encode(&self.client_sign)),
            ("receipt_lender_sign", encode(&self.lender_sign)),
            ("receipt_payment_footer", encode(&self.payment_footer)),
            ("receipt_loan_footer", encode(&self.loan_footer)),
        ];
        for set in RuleSet::ALL {
            let v = self.rules[set.index()].as_ref().map(|lines| {
                let body: Vec<String> = lines.iter().map(|l| format!("{}\t{}", l.when.key(), one_line(&l.text))).collect();
                format!("{CUSTOM}{}", body.join("\n"))
            });
            out.push((set.key(), v.unwrap_or_default()));
        }
        out
    }

    /// Checks lengths before saving. Returns the problem to show.
    pub fn problem(&self) -> Option<&'static str> {
        let too_long = |v: &Option<String>, max: usize| v.as_ref().is_some_and(|s| s.chars().count() > max);
        if too_long(&self.heading, 80) {
            return Some(t("The heading is too long (80 characters at most)", "El título es muy largo (máximo 80 caracteres)"));
        }
        if too_long(&self.client_sign, 80) || too_long(&self.lender_sign, 80) {
            return Some(t(
                "A signature caption is too long (80 characters at most)",
                "Un texto de firma es muy largo (máximo 80 caracteres)",
            ));
        }
        if too_long(&self.payment_footer, 300) || too_long(&self.loan_footer, 300) {
            return Some(t("A closing line is too long (300 characters at most)", "Una línea final es muy larga (máximo 300 caracteres)"));
        }
        for lines in self.rules.iter().flatten() {
            if lines.len() > 20 {
                return Some(t("Use 20 lines at most", "Usa 20 líneas como máximo"));
            }
            if lines.iter().any(|l| l.text.chars().count() > 500) {
                return Some(t("A line is too long (500 characters at most)", "Una línea es muy larga (máximo 500 caracteres)"));
            }
        }
        None
    }
}

/// Values for the tags in one receipt.
#[derive(Clone, Debug, Default)]
pub struct Tags {
    pub amount: String,
    pub interest: String,
    pub late_fee: String,
    pub conversion: String,
    pub example_payment: String,
    pub example_capital: String,
    pub next_interest: String,
    pub company: String,
}

impl Tags {
    pub fn fill(&self, text: &str) -> String {
        [
            ("{amount}", &self.amount),
            ("{interest}", &self.interest),
            ("{late_fee}", &self.late_fee),
            ("{conversion}", &self.conversion),
            ("{example_payment}", &self.example_payment),
            ("{example_capital}", &self.example_capital),
            ("{next_interest}", &self.next_interest),
            ("{company}", &self.company),
        ]
        .iter()
        .fold(text.to_string(), |s, (tag, v)| s.replace(tag, v))
    }
}

/// The lines a loan's receipt prints: those that apply to it, with tags
/// filled in. Lines left empty are skipped.
pub fn lines_for(lines: &[RuleLine], tags: &Tags, converted: bool, late_fee: bool, edited: bool) -> Vec<String> {
    lines
        .iter()
        .filter(|l| match l.when {
            When::Always => true,
            When::Converted => converted,
            When::LateFee => late_fee,
            When::ScheduleEdited => edited,
        })
        .map(|l| tags.fill(&l.text))
        .filter(|l| !l.trim().is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_reads_back_custom_text() {
        let mut text = ReceiptText {
            heading: Some("Payment rules".into()),
            payment_footer: Some(String::new()),
            lender_sign: Some("Signed for\t{company}".into()),
            ..Default::default()
        };
        text.set_rules(
            RuleSet::Flat,
            Some(vec![RuleLine::new(When::LateFee, "Late: {late_fee}"), RuleLine::new(When::Always, "Pay on time.\nThanks")]),
        );
        let pairs: std::collections::HashMap<_, _> = text.to_pairs().into_iter().collect();
        let back = ReceiptText::from_map(|k| pairs.get(k).cloned());
        assert_eq!(back.heading.as_deref(), Some("Payment rules"));
        // An emptied line stays empty instead of returning to the default.
        assert_eq!(back.payment_footer.as_deref(), Some(""));
        assert_eq!(back.lender_sign.as_deref(), Some("Signed for {company}"));
        let flat = back.rules[RuleSet::Flat.index()].clone().unwrap();
        assert_eq!(flat, vec![RuleLine::new(When::LateFee, "Late: {late_fee}"), RuleLine::new(When::Always, "Pay on time. Thanks")]);
        assert_eq!(back.rules[RuleSet::Declining.index()], None);
        assert!(ReceiptText::from_map(|_| None).is_default());
    }

    #[test]
    fn conditional_lines_print_only_when_they_apply() {
        let tags = Tags { late_fee: "5%".into(), conversion: "x".into(), ..Default::default() };
        let lines = default_rules(RuleSet::Declining);
        assert_eq!(lines_for(&lines, &tags, false, false, false).len(), 2);
        let all = lines_for(&lines, &tags, true, true, true);
        assert_eq!(all.len(), 5);
        assert!(all[2].contains("late fee of 5% of what"));
    }
}
