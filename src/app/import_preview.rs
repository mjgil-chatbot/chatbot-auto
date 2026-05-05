use eframe::egui;
pub(super) const LEGACY_IMPORT_HISTORY_LIMIT: usize = 8;
pub(super) const LEGACY_IMPORT_HISTORY_COMPACT_LIMIT: usize = 3;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ImportSummary {
    pub(super) processed: usize,
    pub(super) added: usize,
    pub(super) updated: usize,
    pub(super) unchanged: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImportPreviewKind {
    Add,
    Update,
    Unchanged,
}
impl ImportPreviewKind {
    pub(super) fn metadata(self) -> (&'static str, &'static str) {
        match self {
            Self::Add => ("Add", "new target"),
            Self::Update => ("Update", "managed fields changed"),
            Self::Unchanged => ("Unchanged", "already matches import"),
        }
    }}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ImportPreviewFilter {
    #[default]
    All,
    Add,
    Update,
    Unchanged,
}
impl ImportPreviewFilter {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Add => "Add",
            Self::Update => "Update",
            Self::Unchanged => "Unchanged",
        }
    }}
impl ImportPreviewFilter {
    pub(super) fn matches(self, kind: ImportPreviewKind) -> bool {
        match self {
            Self::All => true,
            Self::Add => kind == ImportPreviewKind::Add,
            Self::Update => kind == ImportPreviewKind::Update,
            Self::Unchanged => kind == ImportPreviewKind::Unchanged,
        }
    }}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ImportPreviewSort {
    #[default]
    KindThenName,
    ProviderThenName,
    Name,
}
impl ImportPreviewSort {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::KindThenName => "Kind then Name",
            Self::ProviderThenName => "Provider then Name",
            Self::Name => "Name",
        }
    }}
