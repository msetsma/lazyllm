/// Convert LaTeX math expressions to Unicode approximations.
///
/// Handles both inline `$...$` and display `$$...$$` math blocks.
/// This is a best-effort conversion — complex expressions may not render
/// perfectly, but common math notation will be readable in a terminal.
pub fn convert_latex(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c == '$' {
            // Check for display math ($$...$$)
            let is_display = chars.peek().is_some_and(|&(_, next)| next == '$');
            if is_display {
                chars.next(); // consume second $
                let start = chars.peek().map(|&(j, _)| j).unwrap_or(input.len());
                if let Some(end) = find_closing_dollars(&input[start..], true) {
                    let math = &input[start..start + end];
                    result.push('\n');
                    result.push_str("  ");
                    result.push_str(&convert_math_expr(math.trim()));
                    result.push('\n');
                    // Skip past closing $$
                    let skip_to = start + end + 2;
                    while chars.peek().is_some_and(|&(j, _)| j < skip_to) {
                        chars.next();
                    }
                    continue;
                }
            }

            // Inline math ($...$)
            let start = chars.peek().map(|&(j, _)| j).unwrap_or(input.len());
            if let Some(end) = find_closing_dollars(&input[start..], false) {
                let math = &input[start..start + end];
                // Don't convert if it looks like a currency amount
                if !math.is_empty() && !math.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
                    result.push_str(&convert_math_expr(math.trim()));
                    let skip_to = start + end + 1;
                    while chars.peek().is_some_and(|&(j, _)| j < skip_to) {
                        chars.next();
                    }
                    continue;
                }
            }

            // Not a math expression, output the $ literally
            result.push('$');
        } else if c == '\\' && i + 1 < input.len() {
            // Standalone LaTeX commands outside of math mode (e.g., \textbf)
            // Just pass through — they're uncommon outside $ delimiters
            result.push(c);
        } else {
            result.push(c);
        }
    }

    result
}

/// Find the position of the closing `$` or `$$`.
/// Returns the byte offset of the closing delimiter relative to the input start.
fn find_closing_dollars(input: &str, display: bool) -> Option<usize> {
    let mut i = 0;
    let bytes = input.as_bytes();

    while i < input.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped characters
            continue;
        }
        if display && i + 1 < input.len() && bytes[i] == b'$' && bytes[i + 1] == b'$' {
            return Some(i);
        }
        if !display && bytes[i] == b'$' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Convert a single math expression (contents between $ delimiters) to Unicode.
fn convert_math_expr(expr: &str) -> String {
    let mut result = String::new();
    let mut chars = expr.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let cmd = collect_command(&mut chars);
                result.push_str(&resolve_command(&cmd));
            }
            '_' => {
                let sub = collect_group(&mut chars);
                for ch in sub.chars() {
                    result.push(to_subscript(ch));
                }
            }
            '^' => {
                let sup = collect_group(&mut chars);
                for ch in sup.chars() {
                    result.push(to_superscript(ch));
                }
            }
            '{' | '}' => {} // skip bare braces
            '~' => result.push(' '),
            _ => result.push(c),
        }
    }

    result
}

/// Collect a LaTeX command name (letters only, stops at non-letter).
fn collect_command(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut cmd = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphabetic() {
            cmd.push(c);
            chars.next();
        } else {
            break;
        }
    }
    // If no alphabetic chars, it's a single-character command like \{ or \\
    if cmd.is_empty() {
        if let Some(c) = chars.next() {
            cmd.push(c);
        }
    }
    // Note: we intentionally do NOT consume trailing spaces after commands.
    // While LaTeX normally consumes them, keeping them produces more readable
    // Unicode output in a terminal (e.g., "α + β" instead of "α+ β").
    cmd
}

/// Collect a group: either `{...}` or a single character.
fn collect_group(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    match chars.peek() {
        Some(&'{') => {
            chars.next(); // consume {
            let mut depth = 1;
            let mut group = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '{' => {
                        depth += 1;
                        group.push(c);
                    }
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        group.push(c);
                    }
                    _ => group.push(c),
                }
            }
            group
        }
        Some(&c) => {
            chars.next();
            c.to_string()
        }
        None => String::new(),
    }
}

/// Map a LaTeX command to its Unicode equivalent.
fn resolve_command(cmd: &str) -> String {
    match cmd {
        // Greek lowercase
        "alpha" => "α".into(),
        "beta" => "β".into(),
        "gamma" => "γ".into(),
        "delta" => "δ".into(),
        "epsilon" | "varepsilon" => "ε".into(),
        "zeta" => "ζ".into(),
        "eta" => "η".into(),
        "theta" | "vartheta" => "θ".into(),
        "iota" => "ι".into(),
        "kappa" => "κ".into(),
        "lambda" => "λ".into(),
        "mu" => "μ".into(),
        "nu" => "ν".into(),
        "xi" => "ξ".into(),
        "pi" => "π".into(),
        "rho" | "varrho" => "ρ".into(),
        "sigma" => "σ".into(),
        "tau" => "τ".into(),
        "upsilon" => "υ".into(),
        "phi" | "varphi" => "φ".into(),
        "chi" => "χ".into(),
        "psi" => "ψ".into(),
        "omega" => "ω".into(),

        // Greek uppercase
        "Gamma" => "Γ".into(),
        "Delta" => "Δ".into(),
        "Theta" => "Θ".into(),
        "Lambda" => "Λ".into(),
        "Xi" => "Ξ".into(),
        "Pi" => "Π".into(),
        "Sigma" => "Σ".into(),
        "Upsilon" => "Υ".into(),
        "Phi" => "Φ".into(),
        "Psi" => "Ψ".into(),
        "Omega" => "Ω".into(),

        // Operators
        "sum" => "∑".into(),
        "prod" => "∏".into(),
        "int" => "∫".into(),
        "iint" => "∬".into(),
        "iiint" => "∭".into(),
        "oint" => "∮".into(),
        "partial" => "∂".into(),
        "nabla" => "∇".into(),
        "sqrt" => "√".into(),
        "cbrt" => "∛".into(),
        "infty" | "infinity" => "∞".into(),

        // Relations
        "leq" | "le" => "≤".into(),
        "geq" | "ge" => "≥".into(),
        "neq" | "ne" => "≠".into(),
        "approx" => "≈".into(),
        "equiv" => "≡".into(),
        "sim" => "∼".into(),
        "simeq" => "≃".into(),
        "cong" => "≅".into(),
        "propto" => "∝".into(),
        "ll" => "≪".into(),
        "gg" => "≫".into(),
        "subset" => "⊂".into(),
        "supset" => "⊃".into(),
        "subseteq" => "⊆".into(),
        "supseteq" => "⊇".into(),
        "in" => "∈".into(),
        "notin" => "∉".into(),
        "ni" => "∋".into(),
        "forall" => "∀".into(),
        "exists" => "∃".into(),
        "nexists" => "∄".into(),

        // Arrows
        "to" | "rightarrow" => "→".into(),
        "leftarrow" => "←".into(),
        "leftrightarrow" => "↔".into(),
        "Rightarrow" | "implies" => "⇒".into(),
        "Leftarrow" => "⇐".into(),
        "Leftrightarrow" | "iff" => "⇔".into(),
        "uparrow" => "↑".into(),
        "downarrow" => "↓".into(),
        "mapsto" => "↦".into(),

        // Logic & sets
        "land" | "wedge" => "∧".into(),
        "lor" | "vee" => "∨".into(),
        "lnot" | "neg" => "¬".into(),
        "cap" => "∩".into(),
        "cup" => "∪".into(),
        "emptyset" | "varnothing" => "∅".into(),
        "setminus" => "∖".into(),

        // Misc symbols
        "times" => "×".into(),
        "div" => "÷".into(),
        "cdot" => "·".into(),
        "cdots" => "⋯".into(),
        "ldots" | "dots" => "…".into(),
        "vdots" => "⋮".into(),
        "ddots" => "⋱".into(),
        "pm" => "±".into(),
        "mp" => "∓".into(),
        "star" => "⋆".into(),
        "circ" => "∘".into(),
        "bullet" => "•".into(),
        "dagger" => "†".into(),
        "ddagger" => "‡".into(),
        "angle" => "∠".into(),
        "triangle" => "△".into(),
        "diamond" => "◇".into(),
        "square" => "□".into(),
        "lfloor" => "⌊".into(),
        "rfloor" => "⌋".into(),
        "lceil" => "⌈".into(),
        "rceil" => "⌉".into(),
        "langle" => "⟨".into(),
        "rangle" => "⟩".into(),
        "prime" => "′".into(),

        // Functions (render as text)
        "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin" | "arccos" | "arctan"
        | "sinh" | "cosh" | "tanh" | "log" | "ln" | "exp" | "lim" | "max" | "min"
        | "sup" | "inf" | "det" | "dim" | "ker" | "gcd" | "lcm" | "deg" | "arg"
        | "hom" | "mod" => cmd.into(),

        // Formatting (extract content)
        "text" | "mathrm" | "textbf" | "textit" | "mathbf" | "mathit" | "mathcal"
        | "mathbb" | "mathfrak" => String::new(), // content follows as group

        // Fractions: \frac{a}{b} → a/b
        "frac" => {
            // The actual content will be handled by the caller
            "/".into()
        }

        // Spacing commands
        "quad" => "  ".into(),
        "qquad" => "    ".into(),
        " " => " ".into(),
        "," => " ".into(),
        ";" => " ".into(),
        "!" => String::new(),

        // Escaped characters
        "{" => "{".into(),
        "}" => "}".into(),
        "\\" => "\n".into(),
        "&" => "&".into(),
        "%" => "%".into(),
        "#" => "#".into(),
        "_" => "_".into(),

        // Unknown command — render as-is with backslash
        other => format!("\\{other}"),
    }
}

/// Convert a character to its Unicode superscript equivalent.
fn to_superscript(c: char) -> char {
    match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'T' => 'ᵀ',
        _ => c,
    }
}

/// Convert a character to its Unicode subscript equivalent.
fn to_subscript(c: char) -> char {
    match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensures Greek letter LaTeX commands (e.g. \alpha, \Sigma) convert to Unicode.
    #[test]
    fn greek_letters() {
        assert_eq!(convert_latex("$\\alpha + \\beta$"), "α + β");
        assert_eq!(convert_latex("$\\Sigma$"), "Σ");
    }

    /// Verifies single-char and braced subscript/superscript notation converts to Unicode.
    #[test]
    fn subscripts_and_superscripts() {
        assert_eq!(convert_latex("$x^2$"), "x²");
        assert_eq!(convert_latex("$x_i$"), "xᵢ");
        assert_eq!(convert_latex("$x^{10}$"), "x¹⁰");
        assert_eq!(convert_latex("$a_{ij}$"), "aᵢⱼ");
    }

    /// Ensures math operators (\sum, \int, \infty, \partial) convert to Unicode symbols.
    #[test]
    fn operators() {
        assert_eq!(convert_latex("$\\sum$"), "∑");
        assert_eq!(convert_latex("$\\int$"), "∫");
        assert_eq!(convert_latex("$\\infty$"), "∞");
        assert_eq!(convert_latex("$\\partial$"), "∂");
    }

    /// Verifies relational operators (\leq, \neq, \approx, \in) convert to Unicode.
    #[test]
    fn relations() {
        assert_eq!(convert_latex("$a \\leq b$"), "a ≤ b");
        assert_eq!(convert_latex("$x \\neq y$"), "x ≠ y");
        assert_eq!(convert_latex("$x \\approx y$"), "x ≈ y");
        assert_eq!(convert_latex("$x \\in S$"), "x ∈ S");
    }

    /// Ensures arrow commands (\to, \implies, \iff) convert to Unicode arrows.
    #[test]
    fn arrows() {
        assert_eq!(convert_latex("$a \\to b$"), "a → b");
        assert_eq!(convert_latex("$A \\implies B$"), "A ⇒ B");
        assert_eq!(convert_latex("$A \\iff B$"), "A ⇔ B");
    }

    /// Verifies display math ($$...$$) is converted with surrounding text preserved.
    #[test]
    fn display_math() {
        let result = convert_latex("text $$x^2 + y^2$$ more");
        assert!(result.contains("x² + y²"));
    }

    /// Ensures complex expressions with limits (\sum_{i=1}^{n} x_i) render all parts.
    #[test]
    fn sum_with_limits() {
        let result = convert_latex("$\\sum_{i=1}^{n} x_i$");
        assert!(result.contains("∑"));
        assert!(result.contains("ᵢ₌₁"));
        assert!(result.contains("ⁿ"));
        assert!(result.contains("xᵢ"));
    }

    /// Ensures text without dollar signs passes through unconverted.
    #[test]
    fn no_conversion_outside_dollars() {
        assert_eq!(convert_latex("plain text"), "plain text");
        assert_eq!(convert_latex("no math here"), "no math here");
    }

    /// Verifies dollar amounts (e.g. $100) are not treated as math mode.
    #[test]
    fn preserves_currency() {
        // Dollar amounts should not be converted
        assert_eq!(convert_latex("costs $100"), "costs $100");
    }

    /// Ensures trigonometric functions (\sin, \cos) render as plain text names.
    #[test]
    fn trig_functions() {
        assert_eq!(convert_latex("$\\sin(x)$"), "sin(x)");
        assert_eq!(convert_latex("$\\cos(\\theta)$"), "cos(θ)");
    }

    /// Verifies inline math converts while surrounding prose is preserved.
    #[test]
    fn mixed_text_and_math() {
        let input = "The equation $E = mc^2$ is famous.";
        let result = convert_latex(input);
        assert!(result.contains("E = mc²"));
        assert!(result.contains("The equation"));
        assert!(result.contains("is famous."));
    }

    /// Ensures logic quantifiers (\forall, \exists) convert to Unicode.
    #[test]
    fn logic_symbols() {
        assert_eq!(convert_latex("$\\forall x \\exists y$"), "∀ x ∃ y");
    }

    /// Verifies set operation commands (\cup, \cap, \emptyset) convert to Unicode.
    #[test]
    fn set_operations() {
        assert_eq!(convert_latex("$A \\cup B$"), "A ∪ B");
        assert_eq!(convert_latex("$A \\cap B$"), "A ∩ B");
        assert_eq!(convert_latex("$\\emptyset$"), "∅");
    }

    /// Ensures \sqrt{x} converts to the √ symbol followed by the argument.
    #[test]
    fn sqrt_symbol() {
        assert_eq!(convert_latex("$\\sqrt{x}$"), "√x");
    }

    /// Verifies miscellaneous symbols (\times, \cdot, \pm) convert to Unicode.
    #[test]
    fn misc_symbols() {
        assert_eq!(convert_latex("$a \\times b$"), "a × b");
        assert_eq!(convert_latex("$a \\cdot b$"), "a · b");
        assert_eq!(convert_latex("$a \\pm b$"), "a ± b");
    }

    /// Ensures unmatched dollar signs pass through without conversion.
    #[test]
    fn escaped_dollar_passthrough() {
        // Unmatched $ should pass through
        assert_eq!(convert_latex("just a $ sign"), "just a $ sign");
    }
}
