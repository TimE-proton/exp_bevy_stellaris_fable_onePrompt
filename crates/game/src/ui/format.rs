//! Zahlen und Einheiten in deutscher Schreibweise.

/// Dezimalzahl mit Komma.
pub fn decimal(value: f64, places: usize) -> String {
    format!("{value:.places$}").replace('.', ",")
}

/// Bevölkerung, übergeben in Millionen.
pub fn population(millions: f64) -> String {
    if millions >= 1000.0 {
        format!("{} Mrd.", decimal(millions / 1000.0, 2))
    } else if millions >= 10.0 {
        format!("{} Mio.", decimal(millions, 0))
    } else {
        format!("{} Mio.", decimal(millions, 1))
    }
}

/// Ganze Zahl mit Tausenderpunkten: „12.000“.
pub fn grouped(value: f64) -> String {
    let digits = format!("{:.0}", value.abs());
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push('.');
        }
        text.push(digit);
    }
    if value < -0.5 {
        text.insert(0, '−');
    }
    text
}

pub fn percent(fraction: f32) -> String {
    format!("{} %", (fraction * 100.0).round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_number_format() {
        assert_eq!(decimal(1.25, 1), "1,2");
        assert_eq!(population(4000.0), "4,00 Mrd.");
        assert_eq!(population(250.4), "250 Mio.");
        assert_eq!(population(2.34), "2,3 Mio.");
        assert_eq!(percent(0.456), "46 %");
        assert_eq!(grouped(0.0), "0");
        assert_eq!(grouped(999.4), "999");
        assert_eq!(grouped(3000.0), "3.000");
        assert_eq!(grouped(400_000.0), "400.000");
        assert_eq!(grouped(1_234_567.0), "1.234.567");
        assert_eq!(grouped(-54_000.0), "−54.000");
    }
}
