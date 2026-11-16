//! GitHub-inspired, terminal-native CMP interaction.
//! Choices are keyboard driven; Ctrl-C and Escape cancel without writes.
//! Noninteractive commands never invoke this module.
use super::Result;
use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

pub fn choose(question: &str, values: &[&str], default: usize) -> Result<usize> {
    if values.is_empty() || default >= values.len() {
        return Err("Invalid CMP selection".into());
    }
    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(question)
        .items(values)
        .default(default)
        .interact_opt()?
        .ok_or("CMP selection cancelled")?;
    Ok(choice)
}

pub fn confirm(question: &str, default: bool) -> Result<bool> {
    Ok(Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(question)
        .default(default)
        .interact_opt()?
        .ok_or("CMP confirmation cancelled")?)
}

pub fn input(question: &str, default: &str) -> Result<String> {
    Ok(Input::<String>::with_theme(&ColorfulTheme::default())
        .with_prompt(question)
        .with_initial_text(default)
        .allow_empty(true)
        .interact_text()?
        .trim()
        .to_owned())
}

pub fn success(message: impl std::fmt::Display) {
    println!("{} {message}", style("✓").green());
}

#[cfg(test)]
mod tests {
    #[test]
    fn rejects_invalid_arrow_menu_without_needing_a_terminal() {
        assert!(super::choose("choose", &[], 0).is_err());
        assert!(super::choose("choose", &["Cloud"], 1).is_err());
    }
}
