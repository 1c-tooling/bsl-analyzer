use crate::common_module::CommonModule;
use crate::enums::ReturnValueReuse;
use crate::error::{MetadataError, Result};
use crate::traits::MdObject;

use super::helpers::{
    child_bool, child_text, find_child, find_mdo_element, parse_extension_ownership, parse_uuid,
    parse_xml,
};

pub fn parse_common_module_xml(xml: &str) -> Result<CommonModule> {
    let _span = tracing::debug_span!("parse_common_module_xml").entered();

    let doc = parse_xml(xml)?;
    let mdo = find_mdo_element(&doc)
        .ok_or_else(|| MetadataError::InvalidFormat("No CommonModule element found".to_string()))?;

    let uuid_str = mdo.attribute("uuid").unwrap_or("");
    let uuid = parse_uuid(uuid_str, "common module")?;

    let props = find_child(mdo, "Properties").ok_or_else(|| {
        MetadataError::InvalidFormat("CommonModule missing Properties".to_string())
    })?;

    let name = child_text(props, "Name").unwrap_or("").to_string();
    let (object_belonging, extended_configuration_object) = parse_extension_ownership(props);

    // A setter is called only for a present element: an absent one stays unset
    // so a borrowed module inherits it from the base configuration.
    let bool_prop = |tag| find_child(props, tag).map(|_| child_bool(props, tag));
    let mut builder = CommonModule::builder()
        .uuid(uuid)
        .name(name)
        .object_belonging(object_belonging)
        .extended_configuration_object(extended_configuration_object);
    if let Some(value) = bool_prop("Server") {
        builder = builder.server(value);
    }
    if let Some(value) = bool_prop("Global") {
        builder = builder.global(value);
    }
    if let Some(value) = bool_prop("ClientManagedApplication") {
        builder = builder.client_managed_application(value);
    }
    if let Some(value) = bool_prop("ClientOrdinaryApplication") {
        builder = builder.client_ordinary_application(value);
    }
    if let Some(value) = bool_prop("ExternalConnection") {
        builder = builder.external_connection(value);
    }
    if let Some(value) = bool_prop("ServerCall") {
        builder = builder.server_call(value);
    }
    if let Some(value) = bool_prop("Privileged") {
        builder = builder.privileged(value);
    }
    if let Some(node) = find_child(props, "ReturnValuesReuse") {
        builder =
            builder.return_values_reuse(ReturnValueReuse::from_name(node.text().unwrap_or("")));
    }
    let module = builder.build();

    tracing::debug!(
        module_name = %module.name(),
        uuid = %module.uuid(),
        server = module.is_server(),
        global = module.is_global(),
        "parsed common module"
    );

    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::ObjectBelonging;

    #[test]
    fn adopted_module_keeps_base_uuid_and_omitted_reuse_as_overlay_metadata() {
        let module = parse_common_module_xml(
            r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
                <CommonModule uuid="11111111-1111-1111-1111-111111111111">
                    <Properties>
                        <Name>Переопределяемый</Name>
                        <ObjectBelonging>Adopted</ObjectBelonging>
                        <ExtendedConfigurationObject>22222222-2222-2222-2222-222222222222</ExtendedConfigurationObject>
                    </Properties>
                </CommonModule>
            </MetaDataObject>"#,
        )
        .unwrap();

        assert_eq!(module.object_belonging(), ObjectBelonging::Adopted);
        assert_eq!(
            module.extends_uuid().unwrap().to_string(),
            "22222222-2222-2222-2222-222222222222"
        );
        assert_eq!(module.return_values_reuse(), ReturnValueReuse::Unknown);
    }

    #[test]
    fn malformed_base_reference_keeps_the_module_loadable() {
        let module = parse_common_module_xml(
            r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
                <CommonModule uuid="11111111-1111-1111-1111-111111111111">
                    <Properties>
                        <Name>М</Name>
                        <ObjectBelonging>Adopted</ObjectBelonging>
                        <ExtendedConfigurationObject>not-a-uuid</ExtendedConfigurationObject>
                    </Properties>
                </CommonModule>
            </MetaDataObject>"#,
        )
        .expect("a malformed base reference must not drop the module");

        assert_eq!(module.object_belonging(), ObjectBelonging::Adopted);
        assert!(module.extends_uuid().is_none());
    }

    fn xml(properties: &str) -> String {
        format!(
            r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses">
<CommonModule uuid="15500000-0000-0000-0000-000000000300">
<Properties><Name>Сервер</Name>{properties}</Properties>
</CommonModule></MetaDataObject>"#
        )
    }

    fn all_true_base() -> CommonModule {
        CommonModule::builder()
            .name("Сервер")
            .server(true)
            .global(true)
            .client_managed_application(true)
            .client_ordinary_application(true)
            .external_connection(true)
            .server_call(true)
            .privileged(true)
            .return_values_reuse(ReturnValueReuse::DontUse)
            .build()
    }

    #[test]
    fn extension_metadata_parser_keeps_absence_distinct_from_explicit_values() {
        let missing = parse_common_module_xml(&xml("")).unwrap();
        let mut inherited = all_true_base();
        inherited.apply_extension_overlay(&missing);
        assert!(inherited.is_server());
        assert!(inherited.is_global());
        assert!(inherited.is_client_managed_application());
        assert!(inherited.is_client_ordinary_application());
        assert!(inherited.is_external_connection());
        assert!(inherited.is_server_call());
        assert!(inherited.is_privileged());
        assert_eq!(inherited.return_values_reuse(), ReturnValueReuse::DontUse);

        let tags = [
            "Server",
            "Global",
            "ClientManagedApplication",
            "ClientOrdinaryApplication",
            "ExternalConnection",
            "ServerCall",
            "Privileged",
        ];
        for omitted in 0..tags.len() {
            let properties = tags
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != omitted)
                .map(|(_, tag)| format!("<{tag}>false</{tag}>"))
                .collect::<String>();
            let overlay = parse_common_module_xml(&xml(&properties)).unwrap();
            let mut effective = all_true_base();
            effective.apply_extension_overlay(&overlay);
            let values = [
                effective.is_server(),
                effective.is_global(),
                effective.is_client_managed_application(),
                effective.is_client_ordinary_application(),
                effective.is_external_connection(),
                effective.is_server_call(),
                effective.is_privileged(),
            ];
            for (index, value) in values.into_iter().enumerate() {
                assert_eq!(
                    value,
                    index == omitted,
                    "omitted XML bool index {omitted}, checked {index}"
                );
            }
        }

        let explicit =
            parse_common_module_xml(&xml("<Server>false</Server><Global>false</Global>\
             <ClientManagedApplication>false</ClientManagedApplication>\
             <ClientOrdinaryApplication>false</ClientOrdinaryApplication>\
             <ExternalConnection>false</ExternalConnection><ServerCall>false</ServerCall>\
             <Privileged>false</Privileged><ReturnValuesReuse>unexpected</ReturnValuesReuse>"))
            .unwrap();
        let mut overridden = all_true_base();
        overridden.apply_extension_overlay(&explicit);
        assert!(!overridden.is_server());
        assert!(!overridden.is_global());
        assert!(!overridden.is_client_managed_application());
        assert!(!overridden.is_client_ordinary_application());
        assert!(!overridden.is_external_connection());
        assert!(!overridden.is_server_call());
        assert!(!overridden.is_privileged());
        assert_eq!(overridden.return_values_reuse(), ReturnValueReuse::Unknown);

        let empty_reuse =
            parse_common_module_xml(&xml("<ReturnValuesReuse/>")).expect("empty element parses");
        let mut empty_overridden = all_true_base();
        empty_overridden.apply_extension_overlay(&empty_reuse);
        assert_eq!(empty_overridden.return_values_reuse(), ReturnValueReuse::Unknown);

        for reuse in ["DuringRequest", "DuringSession"] {
            let properties = format!(
                "<Server>true</Server><Global>true</Global>\
                 <ClientManagedApplication>true</ClientManagedApplication>\
                 <ClientOrdinaryApplication>true</ClientOrdinaryApplication>\
                 <ExternalConnection>true</ExternalConnection><ServerCall>true</ServerCall>\
                 <Privileged>true</Privileged><ReturnValuesReuse>{reuse}</ReturnValuesReuse>"
            );
            let parsed = parse_common_module_xml(&xml(&properties)).unwrap();
            let mut false_base = CommonModule::builder()
                .name("Сервер")
                .server(false)
                .global(false)
                .client_managed_application(false)
                .client_ordinary_application(false)
                .external_connection(false)
                .server_call(false)
                .privileged(false)
                .return_values_reuse(ReturnValueReuse::DontUse)
                .build();
            false_base.apply_extension_overlay(&parsed);
            assert!(false_base.is_server());
            assert!(false_base.is_global());
            assert!(false_base.is_client_managed_application());
            assert!(false_base.is_client_ordinary_application());
            assert!(false_base.is_external_connection());
            assert!(false_base.is_server_call());
            assert!(false_base.is_privileged());
            assert_ne!(false_base.return_values_reuse(), ReturnValueReuse::DontUse);
        }
    }
}
