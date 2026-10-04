/// Why control may move along an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CfgEdgeType {
    /// A transfer no value decides: sequence, `Перейти`, `Возврат`, `Прервать`,
    /// `Продолжить` and the end of a loop body returning to its header.
    Unconditional,

    /// The tested condition holds: `Если`/`ИначеЕсли`, a loop continuation, or
    /// a preprocessor directive whose expression holds.
    TrueBranch,

    /// The tested condition does not hold.
    FalseBranch,

    /// Control reaching an exception handler, or leaving the method when no
    /// handler is active: from a `Попытка` to its handler, and from
    /// `ВызватьИсключение`.
    Exception,

    /// Links text that follows a statement which never falls through to that
    /// text. No execution takes it: analyses use it to locate dead statements
    /// and give the state carried over it the bottom value.
    Unexecutable,
}

impl CfgEdgeType {
    pub fn is_executable(&self) -> bool {
        !matches!(self, CfgEdgeType::Unexecutable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_unexecutable_link_is_not_executable() {
        assert!(!CfgEdgeType::Unexecutable.is_executable());
        assert!(CfgEdgeType::Unconditional.is_executable());
        assert!(CfgEdgeType::TrueBranch.is_executable());
        assert!(CfgEdgeType::FalseBranch.is_executable());
        assert!(CfgEdgeType::Exception.is_executable());
    }
}
