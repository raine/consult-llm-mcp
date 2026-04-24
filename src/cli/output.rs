pub fn print_response(model: &str, thread_id: Option<&str>, body: &str) {
    match thread_id {
        Some(id) => println!("[model:{model}] [thread_id:{id}]\n\n{body}"),
        None => println!("[model:{model}]\n\n{body}"),
    }
}

pub fn print_web_confirmation(file_count: usize) {
    println!(
        "\u{2713} Prompt copied to clipboard ({file_count} file{} included).",
        if file_count == 1 { "" } else { "s" }
    );
}
