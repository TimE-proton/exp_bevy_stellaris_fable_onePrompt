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
        text.insert(0, '-');
    }
    text
}

pub fn percent(fraction: f32) -> String {
    format!("{} %", (fraction * 100.0).round() as i32)
}

/// Schreibt Dezimalzahlen eines Textes mit Komma: „3.0 Verbände“ wird „3,0 Verbände“. Der
/// Kern nennt in seinen Gründen Zahlen mit Punkt; die Oberfläche schreibt deutsch. Ein
/// Datum („05.01.2426“), eine Ordnungszahl („1. Flotte“) und Zahlen mit mehr als zwei
/// Nachkommastellen bleiben, wie sie sind.
pub fn german_numbers(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if !chars[index].is_ascii_digit() {
            out.push(chars[index]);
            index += 1;
            continue;
        }
        // Ein Stück aus Ziffern und Punkten, das mit einer Ziffer beginnt.
        let start = index;
        while index < chars.len() && (chars[index].is_ascii_digit() || chars[index] == '.') {
            index += 1;
        }
        // Ein Punkt am Ende gehört zum Satz, nicht zur Zahl.
        while chars[index - 1] == '.' {
            index -= 1;
        }
        let piece: String = chars[start..index].iter().collect();
        let after_word = start > 0 && chars[start - 1].is_alphanumeric();
        match piece.split_once('.') {
            Some((whole, fraction))
                if !after_word && !fraction.contains('.') && fraction.len() <= 2 =>
            {
                out.push_str(whole);
                out.push(',');
                out.push_str(fraction);
            }
            _ => out.push_str(&piece),
        }
    }
    out
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
        // Zahlen aus den Gründen des Kerns: Komma statt Punkt, sonst nichts.
        assert_eq!(
            german_numbers("an Bord von 1. Flotte (Karawane) sind nur 3.0 Verbände."),
            "an Bord von 1. Flotte (Karawane) sind nur 3,0 Verbände."
        );
        assert_eq!(
            german_numbers("nur 0.5 freie Plätze"),
            "nur 0,5 freie Plätze"
        );
        assert_eq!(german_numbers("bis zum 05.01.2426."), "bis zum 05.01.2426.");
        assert_eq!(
            german_numbers("3.000 Credits, Version 1.2.3"),
            "3.000 Credits, Version 1.2.3"
        );
        assert_eq!(german_numbers("Stufe 2."), "Stufe 2.");
        assert_eq!(german_numbers("Yroa-7.5 bleibt"), "Yroa-7,5 bleibt");
        assert_eq!(german_numbers("Typ A7.5 bleibt"), "Typ A7.5 bleibt");
        assert_eq!(grouped(400_000.0), "400.000");
        assert_eq!(grouped(1_234_567.0), "1.234.567");
        assert_eq!(grouped(-54_000.0), "-54.000");
    }
}
