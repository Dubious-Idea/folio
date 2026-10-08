use std::fmt;

pub type Result<T, E = FolioError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum FolioError {
    #[error("Пространство «{0}» не найдено")]
    SpaceNotFound(String),
    #[error("Папка {0} не пространство Folio: в ней нет .folio\\space.json")]
    NotASpace(String),
    #[error("Пространство «{0}» уже подключено")]
    SpaceAlreadyRegistered(String),
    #[error("Папка {0} не пустая: новое пространство создаётся только в пустой папке")]
    FolderNotEmpty(String),
    #[error("Страница «{0}» не найдена")]
    PageNotFound(String),
    #[error("Заголовок «{title}» встречается несколько раз: {}", .paths.join(", "))]
    AmbiguousTitle { title: String, paths: Vec<String> },
    #[error("Страница «{0}» уже есть в этом пространстве, а заголовки должны быть уникальными")]
    DuplicateTitle(String),
    #[error("Недопустимый заголовок «{title}»: {reason}")]
    InvalidTitle { title: String, reason: String },
    #[error("Главную страницу пространства нельзя переносить, переименовывать папкой или удалять")]
    HomePageIsFixed,
    #[error("Нельзя перенести страницу внутрь неё самой")]
    MoveIntoItself,
    #[error("Шаблон «{0}» не найден в папке _Шаблоны")]
    TemplateNotFound(String),
    #[error("{command}: {message}")]
    CommandFailed { command: String, message: String },
    #[error("Индекс поиска: {0}")]
    Database(String),
    #[error("История: {0}")]
    Git(String),
    #[error("{context}: {source}")]
    Io { context: String, source: std::io::Error },
}

impl FolioError {
    pub fn io(context: impl fmt::Display, source: std::io::Error) -> Self {
        FolioError::Io { context: context.to_string(), source }
    }
}

impl From<rusqlite::Error> for FolioError {
    fn from(error: rusqlite::Error) -> Self {
        FolioError::Database(error.to_string())
    }
}

impl From<git2::Error> for FolioError {
    fn from(error: git2::Error) -> Self {
        FolioError::Git(error.message().to_owned())
    }
}

/// Добавляет к ошибке ввода-вывода путь, на котором она случилась.
pub(crate) trait IoContext<T> {
    fn at(self, path: &std::path::Path) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: &std::path::Path) -> Result<T> {
        self.map_err(|error| FolioError::io(path.display(), error))
    }
}
