use indicatif::ProgressStyle;

// styles stolen from omicron-package

pub(crate) fn running_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
        .expect("Invalid template")
        .progress_chars("#>.")
}

pub(crate) fn completed_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg:.green}")
        .expect("Invalid template")
        .progress_chars("#>.")
}
