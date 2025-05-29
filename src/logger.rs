use paris::Logger;

#[allow(unused)]
fn get_logger() -> Logger<'static> {
    Logger::new()
}

pub fn log_info(message: &str) {
    let mut logger = get_logger();
    logger.info(&format!("<blue>{}</>", message));
}

pub fn log_error(message: &str) {
    let mut logger = get_logger();
    logger.error(&format!("<red>{}</>", message));
}

pub fn log_warning(message: &str) {
    let mut logger = get_logger();
    logger.warn(&format!("<yellow>{}</>", message));
}
