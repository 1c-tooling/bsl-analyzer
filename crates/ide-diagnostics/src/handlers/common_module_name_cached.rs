use crate::define_common_module_name_check;
use crate::metadata::*;

define_common_module_name_check! {
    code: CommonModuleNameCached,
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice, MetadataTag::Unpredictable],
    clean_code_attribute: CleanCodeAttribute::Consistent,
    predicate: |m: &bsl_metadata::CommonModule, _oas| {
        m.return_values_reuse() != bsl_metadata::ReturnValueReuse::DontUse
    },
    keywords: &["повторноеиспользование", "повтисп", "cached"],
    name_should_contain: true,
    message: "Имя кэшируемого общего модуля должно содержать 'ПовтИсп' или 'Cached'",
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;

    fn check_cfe_cached_diagnostic(extension_reuse: Option<&str>) -> Vec<crate::Diagnostic> {
        use crate::test_utils::check_cfe_at_with_unreadable_config_and_setup;
        use test_fixture::CfeFixtureBuilder;

        let mut builder = CfeFixtureBuilder::new("");
        builder
            .add_base_module("CachePolicy", "Процедура М() Экспорт\nКонецПроцедуры")
            .add_extension("Расширение", "")
            .add_extension_module(
                "Расширение",
                "CachePolicy",
                "Процедура М() Экспорт\nКонецПроцедуры",
            );
        let fixture = builder.build();

        check_cfe_at_with_unreadable_config_and_setup(
            "CommonModules/CachePolicy/Ext/Module.bsl",
            "Процедура М() Экспорт\nКонецПроцедуры",
            fixture,
            &[],
            crate::DiagnosticsConfig::all_enabled(),
            |fixture| {
                let base_uuid = "00000000-0000-0000-0000-000000000001";
                let reuse = extension_reuse
                    .map(|value| format!("<ReturnValuesReuse>{value}</ReturnValuesReuse>"))
                    .unwrap_or_default();
                let extension_xml = format!(
                    r#"<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.20">
<CommonModule uuid="00000000-0000-0000-0000-000000000002"><Properties>
<Name>CachePolicy</Name><ObjectBelonging>Adopted</ObjectBelonging>
<ExtendedConfigurationObject>{base_uuid}</ExtendedConfigurationObject>
<Server>true</Server>{reuse}
</Properties></CommonModule></MetaDataObject>"#
                );
                std::fs::write(
                    fixture.extensions()[0].root().join("CommonModules/CachePolicy.xml"),
                    extension_xml,
                )
                .expect("write adopted common module metadata");
            },
            |ctx| super::from_metadata(&ctx.module_metadata(), ctx),
        )
    }

    #[test]
    fn test_cached_without_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(bsl_metadata::ReturnValueReuse::DuringRequest)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_cached_with_keyword() {
        let module = bsl_metadata::CommonModule::builder()
            .name("SomethingCached")
            .return_values_reuse(bsl_metadata::ReturnValueReuse::DuringSession)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_not_cached() {
        let module = bsl_metadata::CommonModule::builder()
            .name("Something")
            .return_values_reuse(bsl_metadata::ReturnValueReuse::DontUse)
            .build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn adopted_module_name_cached_uses_effective_return_values_reuse() {
        assert!(
            check_cfe_cached_diagnostic(None).is_empty(),
            "omitted adopted setting inherits base DontUse and must not produce the cached-name diagnostic"
        );
        assert_eq!(
            check_cfe_cached_diagnostic(Some("DuringSession")).len(),
            1,
            "an explicit adopted cache setting must produce the diagnostic when the name lacks Cached"
        );
    }
}
