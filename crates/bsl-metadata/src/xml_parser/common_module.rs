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
    let return_values_reuse_str = child_text(props, "ReturnValuesReuse").unwrap_or("");
    let return_values_reuse = ReturnValueReuse::from_name(return_values_reuse_str);

    let module = CommonModule::builder()
        .uuid(uuid)
        .name(name)
        .object_belonging(object_belonging)
        .extended_configuration_object(extended_configuration_object)
        .server(child_bool(props, "Server"))
        .global(child_bool(props, "Global"))
        .client_managed_application(child_bool(props, "ClientManagedApplication"))
        .client_ordinary_application(child_bool(props, "ClientOrdinaryApplication"))
        .external_connection(child_bool(props, "ExternalConnection"))
        .server_call(child_bool(props, "ServerCall"))
        .privileged(child_bool(props, "Privileged"))
        .return_values_reuse(return_values_reuse)
        .build();

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
}
