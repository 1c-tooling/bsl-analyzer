use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabularSection {
    uuid: Uuid,

    name: String,

    #[serde(default)]
    name_en: Option<String>,

    #[serde(default)]
    synonym: Option<String>,

    #[serde(default)]
    attributes: Vec<TabularSectionAttribute>,

    #[serde(default)]
    use_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabularSectionAttribute {
    uuid: Uuid,
    name: String,
    #[serde(default)]
    name_en: Option<String>,
    attr_type: crate::AttributeType,
}

impl TabularSection {
    pub fn new(uuid: Uuid, name: impl Into<String>) -> Self {
        Self {
            uuid,
            name: name.into(),
            name_en: None,
            synonym: None,
            attributes: Vec::new(),
            use_mode: None,
        }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn name_en(&self) -> Option<&str> {
        self.name_en.as_deref()
    }

    pub fn synonym(&self) -> Option<&str> {
        self.synonym.as_deref()
    }

    pub fn attributes(&self) -> &[TabularSectionAttribute] {
        &self.attributes
    }

    pub fn use_mode(&self) -> Option<&str> {
        self.use_mode.as_deref()
    }

    pub fn set_name_en(&mut self, name_en: Option<String>) {
        self.name_en = name_en;
    }

    pub fn set_synonym(&mut self, synonym: Option<String>) {
        self.synonym = synonym;
    }

    pub fn set_attributes(&mut self, attributes: Vec<TabularSectionAttribute>) {
        self.attributes = attributes;
    }

    pub fn set_use_mode(&mut self, use_mode: Option<String>) {
        self.use_mode = use_mode;
    }

    /// Apply an extension overlay (a borrowed tabular section of the same name)
    /// onto this base section. Identity comes from the overlay; the base
    /// attributes stay, an overlay attribute replaces a same-named one or is
    /// added, and optional values absent in the overlay are inherited.
    pub fn apply_extension_overlay(&mut self, overlay: &TabularSection) {
        let inherited = std::mem::replace(self, overlay.clone());
        self.name_en = self.name_en.take().or(inherited.name_en);
        self.synonym = self.synonym.take().or(inherited.synonym);
        self.use_mode = self.use_mode.take().or(inherited.use_mode);

        let mut attributes = inherited.attributes;
        for attr in &overlay.attributes {
            match attributes
                .iter_mut()
                .find(|existing| stdx::case::eq_ignore_case(&existing.name, &attr.name))
            {
                Some(existing) => *existing = attr.clone(),
                None => attributes.push(attr.clone()),
            }
        }
        self.attributes = attributes;
    }

    /// Heap bytes owned by this tabular section: its name strings plus the
    /// backing attribute vec and each attribute's own owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.name_en.as_ref().map_or(0, String::capacity)
            + self.synonym.as_ref().map_or(0, String::capacity)
            + self.use_mode.as_ref().map_or(0, String::capacity)
            + stdx::heap::vec_bytes::<TabularSectionAttribute>(self.attributes.len())
            + self
                .attributes
                .iter()
                .map(TabularSectionAttribute::estimated_heap_size)
                .sum::<usize>()
    }
}

impl TabularSectionAttribute {
    pub fn new(uuid: Uuid, name: impl Into<String>, attr_type: crate::AttributeType) -> Self {
        Self { uuid, name: name.into(), name_en: None, attr_type }
    }

    pub fn uuid(&self) -> &Uuid {
        &self.uuid
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn name_en(&self) -> Option<&str> {
        self.name_en.as_deref()
    }

    pub fn attr_type(&self) -> &crate::AttributeType {
        &self.attr_type
    }

    pub fn set_name_en(&mut self, name_en: Option<String>) {
        self.name_en = name_en;
    }

    /// Heap bytes owned by this attribute: its name strings plus its type's own
    /// owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.name.capacity()
            + self.name_en.as_ref().map_or(0, String::capacity)
            + self.attr_type.estimated_heap_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tabular_section_creation() {
        let uuid = Uuid::new_v4();
        let ts = TabularSection::new(uuid, "Штрихкоды");

        assert_eq!(ts.name(), "Штрихкоды");
        assert_eq!(ts.uuid(), &uuid);
        assert_eq!(ts.name_en(), None);
        assert_eq!(ts.synonym(), None);
        assert_eq!(ts.attributes().len(), 0);
        assert_eq!(ts.use_mode(), None);
    }

    #[test]
    fn test_tabular_section_with_attributes() {
        let uuid = Uuid::new_v4();
        let mut ts = TabularSection::new(uuid, "Штрихкоды");

        let attr_uuid = Uuid::new_v4();
        let attr_type = crate::AttributeType::String { length: Some(13) };
        let attr = TabularSectionAttribute::new(attr_uuid, "Штрихкод", attr_type.clone());

        ts.set_attributes(vec![attr]);

        assert_eq!(ts.attributes().len(), 1);
        assert_eq!(ts.attributes()[0].name(), "Штрихкод");
        assert_eq!(ts.attributes()[0].attr_type(), &attr_type);
    }

    #[test]
    fn test_tabular_section_with_synonym() {
        let uuid = Uuid::new_v4();
        let mut ts = TabularSection::new(uuid, "Штрихкоды");

        ts.set_synonym(Some("Коды товара".to_string()));

        assert_eq!(ts.synonym(), Some("Коды товара"));
    }

    #[test]
    fn extension_metadata_overlay_preserves_base_fields_and_replaces_conflicts() {
        let base_uuid = Uuid::new_v4();
        let overlay_uuid = Uuid::new_v4();
        let mut base = TabularSection::new(base_uuid, "Товары");
        base.set_name_en(Some("Goods".to_string()));
        base.set_synonym(Some("Товары базы".to_string()));
        base.set_use_mode(Some("ForItem".to_string()));
        base.set_attributes(vec![
            TabularSectionAttribute::new(
                Uuid::new_v4(),
                "Номенклатура",
                crate::AttributeType::String { length: Some(50) },
            ),
            TabularSectionAttribute::new(
                Uuid::new_v4(),
                "Количество",
                crate::AttributeType::Number { precision: 10, scale: 0 },
            ),
        ]);

        let mut overlay = TabularSection::new(overlay_uuid, "Товары");
        overlay.set_attributes(vec![
            TabularSectionAttribute::new(
                Uuid::new_v4(),
                "Количество",
                crate::AttributeType::String { length: Some(15) },
            ),
            TabularSectionAttribute::new(
                Uuid::new_v4(),
                "РасшПоле",
                crate::AttributeType::String { length: Some(25) },
            ),
        ]);

        base.apply_extension_overlay(&overlay);

        assert_eq!(base.uuid(), &overlay_uuid, "the borrowed section keeps overlay identity");
        assert_eq!(base.name_en(), Some("Goods"));
        assert_eq!(base.synonym(), Some("Товары базы"));
        assert_eq!(base.use_mode(), Some("ForItem"));
        let names: Vec<_> = base.attributes().iter().map(TabularSectionAttribute::name).collect();
        assert_eq!(names, ["Номенклатура", "Количество", "РасшПоле"]);
        assert!(matches!(
            base.attributes()[1].attr_type(),
            crate::AttributeType::String { length: Some(15) }
        ));

        let mut inherited = base.clone();
        let empty_overlay = TabularSection::new(Uuid::new_v4(), "Товары");
        inherited.apply_extension_overlay(&empty_overlay);
        assert_eq!(
            inherited.attributes().iter().map(TabularSectionAttribute::name).collect::<Vec<_>>(),
            ["Номенклатура", "Количество", "РасшПоле"],
            "an empty borrowed section must not erase inherited fields"
        );

        let mut explicit_overlay = TabularSection::new(Uuid::new_v4(), "Товары");
        explicit_overlay.set_name_en(Some("OverlayGoods".to_string()));
        explicit_overlay.set_synonym(Some("Товары расширения".to_string()));
        explicit_overlay.set_use_mode(Some("ForFolder".to_string()));
        inherited.apply_extension_overlay(&explicit_overlay);
        assert_eq!(inherited.name_en(), Some("OverlayGoods"));
        assert_eq!(inherited.synonym(), Some("Товары расширения"));
        assert_eq!(inherited.use_mode(), Some("ForFolder"));
    }
}
