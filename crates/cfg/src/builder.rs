use crate::edge::CfgEdgeType;
use crate::graph::ControlFlowGraph;
use crate::vertex::{
    BasicBlockVertex, CfgVertex, ConditionalVertex, ForEachHeaderVertex, ForHeaderVertex,
    PreprocConditionVertex, WhileHeaderVertex,
};
use cfg_types::{BindingId, ExprId, IdConversion, StmtId};
use hir_def::hir::StmtIdx;
use hir_def::{Body, Name, Stmt};
use petgraph::graph::NodeIndex;
use rustc_hash::FxHashMap;

struct LoopFrame {
    header: NodeIndex,
    exit: NodeIndex,
}

pub struct CfgBuilder {
    cfg: ControlFlowGraph,
    current_block: Option<NodeIndex>,

    except_stack: Vec<NodeIndex>,

    loop_stack: Vec<LoopFrame>,

    label_table: FxHashMap<Name, NodeIndex>,

    pending_gotos: Vec<(NodeIndex, Name)>,
}

impl CfgBuilder {
    pub fn new() -> Self {
        Self {
            cfg: ControlFlowGraph::new(),
            current_block: None,
            except_stack: Vec::new(),
            loop_stack: Vec::new(),
            label_table: FxHashMap::default(),
            pending_gotos: Vec::new(),
        }
    }

    pub fn build_graph_from_hir(mut self, body_stmts: &[StmtIdx], body: &Body) -> ControlFlowGraph {
        let start = self.new_block();
        self.cfg.set_start(start);
        self.current_block = Some(start);

        for &stmt_id in body_stmts {
            self.walk_statement_hir(stmt_id, body);
        }

        if let Some(block_idx) = self.current_block {
            let exit = self.cfg.exit();
            let ends_with_terminator =
                if let Some(CfgVertex::BasicBlock(bb)) = self.cfg.vertex(block_idx) {
                    bb.statements().last().is_some_and(|&stmt_id| {
                        matches!(body.stmt(stmt_id), Stmt::Return { .. } | Stmt::Raise { .. })
                    })
                } else {
                    false
                };

            if !ends_with_terminator {
                self.link_to(block_idx, exit);
            }
        }

        self.cfg
    }

    fn walk_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        let stmt = body.stmt_idx(stmt_id);
        if let Stmt::Expr(expr_idx) = stmt {
            if body.is_recovered(ExprId::from_idx(*expr_idx)) {
                return;
            }
        }
        match stmt {
            Stmt::Return { .. } => self.walk_return_statement_hir(stmt_id),
            Stmt::Raise { .. } => self.walk_raise_statement_hir(stmt_id),
            Stmt::If(_) => self.walk_if_statement_hir(stmt_id, body),
            Stmt::PreprocIf(_) => self.walk_preproc_if_statement_hir(stmt_id, body),
            Stmt::While { condition, body: loop_body } => self.walk_loop(
                stmt_id,
                CfgVertex::WhileHeader(WhileHeaderVertex::new(ExprId::from_idx(*condition))),
                loop_body,
                body,
            ),
            Stmt::For { var, from, to, body: loop_body } => self.walk_loop(
                stmt_id,
                CfgVertex::ForHeader(ForHeaderVertex::new(
                    BindingId::from_idx(*var),
                    ExprId::from_idx(*from),
                    ExprId::from_idx(*to),
                )),
                loop_body,
                body,
            ),
            Stmt::ForEach { var, collection, body: loop_body } => self.walk_loop(
                stmt_id,
                CfgVertex::ForEachHeader(ForEachHeaderVertex::new(
                    BindingId::from_idx(*var),
                    ExprId::from_idx(*collection),
                )),
                loop_body,
                body,
            ),
            Stmt::Try { .. } => self.walk_try_statement_hir(stmt_id, body),
            Stmt::Break => self.walk_break_statement_hir(stmt_id),
            Stmt::Continue => self.walk_continue_statement_hir(stmt_id),
            Stmt::Goto(_) => self.walk_goto_statement_hir(stmt_id, body),
            Stmt::Label(_) => self.walk_label_statement_hir(stmt_id, body),
            _ => {
                self.add_to_current_block_hir(stmt_id);
            }
        }
    }

    fn new_block(&mut self) -> NodeIndex {
        self.cfg.add_vertex(CfgVertex::BasicBlock(BasicBlockVertex::new()))
    }

    fn add_to_current_block_hir(&mut self, stmt_id: StmtIdx) {
        if let Some(block_idx) = self.current_block {
            if let Some(CfgVertex::BasicBlock(block)) = self.cfg.vertex_mut(block_idx) {
                block.add_statement(StmtId::from_idx(stmt_id));
            }
        }
    }

    /// Continues the text after a statement that never falls through: what
    /// follows lands in a fresh block that execution cannot enter from here.
    fn seal_jump(&mut self) {
        let dead_block = self.new_block();
        if let Some(block_idx) = self.current_block {
            self.cfg.add_edge(block_idx, dead_block, CfgEdgeType::Unexecutable);
        }
        self.current_block = Some(dead_block);
    }

    /// Falls through from `from` to `to`; the link stays unexecutable when
    /// nothing executes `from` in the first place.
    fn link_to(&mut self, from: NodeIndex, to: NodeIndex) {
        let kind = if self.block_has_live_incoming(from) {
            CfgEdgeType::Unconditional
        } else {
            CfgEdgeType::Unexecutable
        };
        self.cfg.add_edge(from, to, kind);
    }

    fn walk_return_statement_hir(&mut self, stmt_id: StmtIdx) {
        self.add_to_current_block_hir(stmt_id);

        if let Some(block_idx) = self.current_block {
            let exit = self.cfg.exit();
            self.cfg.add_edge(block_idx, exit, CfgEdgeType::Unconditional);
            self.seal_jump();
        }
    }

    /// The nearest enclosing handler receives the exception; with none active
    /// the method terminates.
    fn walk_raise_statement_hir(&mut self, stmt_id: StmtIdx) {
        self.add_to_current_block_hir(stmt_id);

        if let Some(block_idx) = self.current_block {
            let target = self.except_stack.last().copied().unwrap_or_else(|| self.cfg.exit());
            self.cfg.add_edge(block_idx, target, CfgEdgeType::Exception);
            self.seal_jump();
        }
    }

    fn walk_break_statement_hir(&mut self, stmt_id: StmtIdx) {
        self.add_to_current_block_hir(stmt_id);

        if let (Some(block_idx), Some(frame)) = (self.current_block, self.loop_stack.last()) {
            self.cfg.add_edge(block_idx, frame.exit, CfgEdgeType::Unconditional);
        }
        self.seal_jump();
    }

    fn walk_continue_statement_hir(&mut self, stmt_id: StmtIdx) {
        self.add_to_current_block_hir(stmt_id);

        if let (Some(block_idx), Some(frame)) = (self.current_block, self.loop_stack.last()) {
            self.cfg.add_edge(block_idx, frame.header, CfgEdgeType::Unconditional);
        }
        self.seal_jump();
    }

    /// A jump to a label not seen yet waits for it; a label never seen leaves
    /// the jump without a target.
    fn walk_goto_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        self.add_to_current_block_hir(stmt_id);

        if let (Some(block_idx), Stmt::Goto(name)) = (self.current_block, body.stmt_idx(stmt_id)) {
            if let Some(&target) = self.label_table.get(name) {
                self.cfg.add_edge(block_idx, target, CfgEdgeType::Unconditional);
            } else {
                self.pending_gotos.push((block_idx, name.clone()));
            }
        }
        self.seal_jump();
    }

    /// A label starts a new basic block: jumps and the preceding text both
    /// enter it, and the label is its only statement. The text after the label
    /// continues in a block of its own: solvers spend their iteration budget
    /// per vertex, so keeping that split keeps a partial result under a given
    /// limit the same.
    fn walk_label_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        if let Stmt::Label(name) = body.stmt_idx(stmt_id) {
            let label_block = self.new_block();

            if let Some(current) = self.current_block {
                self.cfg.add_edge(current, label_block, CfgEdgeType::Unconditional);
            }

            let pending = std::mem::take(&mut self.pending_gotos);
            let (matched, leftover): (Vec<_>, Vec<_>) =
                pending.into_iter().partition(|(_, n)| n == name);
            for (source, _) in matched {
                self.cfg.add_edge(source, label_block, CfgEdgeType::Unconditional);
            }
            self.pending_gotos = leftover;

            self.label_table.insert(name.clone(), label_block);

            self.current_block = Some(label_block);
            self.add_to_current_block_hir(stmt_id);

            let after_label = self.new_block();
            self.cfg.add_edge(label_block, after_label, CfgEdgeType::Unconditional);
            self.current_block = Some(after_label);
        }
    }

    fn is_block_reachable(&self, block: NodeIndex) -> bool {
        let has_incoming = self.cfg.incoming_edges(block).next().is_some();
        let is_start = self.cfg.start() == Some(block);
        has_incoming || is_start
    }

    fn block_has_live_incoming(&self, block: NodeIndex) -> bool {
        if self.cfg.start() == Some(block) {
            return true;
        }
        self.cfg.incoming_edges(block).any(|(_, edge_type)| edge_type.is_executable())
    }

    fn walk_if_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        if let Stmt::If(if_stmt) = body.stmt_idx(stmt_id) {
            let cond_vertex = self.cfg.add_vertex_with_origin(
                CfgVertex::Conditional(ConditionalVertex::new(ExprId::from_idx(if_stmt.condition))),
                StmtId::from_idx(stmt_id),
            );

            if let Some(current) = self.current_block {
                self.cfg.add_edge(current, cond_vertex, CfgEdgeType::Unconditional);
            }

            let merge_block = self.new_block();

            let then_block = self.new_block();
            self.cfg.add_edge(cond_vertex, then_block, CfgEdgeType::TrueBranch);
            self.current_block = Some(then_block);

            for &then_stmt_id in if_stmt.then_branch.iter() {
                self.walk_statement_hir(then_stmt_id, body);
            }

            if let Some(exit) = self.current_block {
                self.link_to(exit, merge_block);
            }

            let mut current_cond = cond_vertex;

            for (elsif_condition, elsif_body) in if_stmt.elsif_branches.iter() {
                let elsif_cond = self.cfg.add_vertex_with_origin(
                    CfgVertex::Conditional(ConditionalVertex::new(ExprId::from_idx(
                        *elsif_condition,
                    ))),
                    StmtId::from_idx(stmt_id),
                );

                self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

                let elsif_block = self.new_block();
                self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
                self.current_block = Some(elsif_block);

                for &elsif_stmt_id in elsif_body.iter() {
                    self.walk_statement_hir(elsif_stmt_id, body);
                }

                if let Some(exit) = self.current_block {
                    self.link_to(exit, merge_block);
                }

                current_cond = elsif_cond;
            }

            if let Some(ref else_stmts) = if_stmt.else_branch {
                let else_block = self.new_block();
                self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
                self.current_block = Some(else_block);

                for &else_stmt_id in else_stmts.iter() {
                    self.walk_statement_hir(else_stmt_id, body);
                }

                if let Some(exit) = self.current_block {
                    self.link_to(exit, merge_block);
                }
            } else {
                self.cfg.add_edge(current_cond, merge_block, CfgEdgeType::FalseBranch);
            }

            self.current_block = Some(merge_block);
        }
    }

    /// Every alternative is analysed: the build environment that selects one
    /// is not known here.
    fn walk_preproc_if_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        use hir_def::hir::HirPreBranchKind;

        if let Stmt::PreprocIf(preproc_if) = body.stmt_idx(stmt_id) {
            let cond_vertex = self.cfg.add_vertex(CfgVertex::PreprocCondition(
                PreprocConditionVertex::with_ranges(
                    preproc_if.condition_range,
                    preproc_if.directive_range,
                    preproc_if.full_range,
                ),
            ));

            if let Some(current) = self.current_block {
                self.cfg.add_edge(current, cond_vertex, CfgEdgeType::Unconditional);
            }

            let merge_block = self.new_block();

            let mut current_cond = cond_vertex;
            let mut saw_else = false;

            for branch in preproc_if.branches() {
                match branch.kind {
                    HirPreBranchKind::Then => {
                        let then_block = self.new_block();
                        self.cfg.add_edge(cond_vertex, then_block, CfgEdgeType::TrueBranch);
                        self.current_block = Some(then_block);
                    }
                    HirPreBranchKind::ElsIf(_) => {
                        let elsif_cond = self.cfg.add_vertex(CfgVertex::PreprocCondition(
                            PreprocConditionVertex::with_directive_range(
                                branch.condition_range.expect("elsif branch has condition range"),
                                branch.directive_range.expect("elsif branch has directive range"),
                            ),
                        ));

                        self.cfg.add_edge(current_cond, elsif_cond, CfgEdgeType::FalseBranch);

                        let elsif_block = self.new_block();
                        self.cfg.add_edge(elsif_cond, elsif_block, CfgEdgeType::TrueBranch);
                        self.current_block = Some(elsif_block);

                        current_cond = elsif_cond;
                    }
                    HirPreBranchKind::Else => {
                        let else_block = self.new_block();
                        self.cfg.add_edge(current_cond, else_block, CfgEdgeType::FalseBranch);
                        self.current_block = Some(else_block);
                        saw_else = true;
                    }
                }

                for &branch_stmt_id in branch.stmts.iter() {
                    self.walk_statement_hir(branch_stmt_id, body);
                }

                if let Some(exit) = self.current_block {
                    self.link_to(exit, merge_block);
                }
            }

            if !saw_else {
                self.cfg.add_edge(current_cond, merge_block, CfgEdgeType::FalseBranch);
            }

            self.current_block = Some(merge_block);
        }
    }

    /// The header holds what the loop computes before each pass and tests the
    /// continuation. The block after the loop exists before the body is walked
    /// because `Прервать` inside the body needs it as a target.
    fn walk_loop(
        &mut self,
        stmt_id: StmtIdx,
        header: CfgVertex,
        loop_body: &[StmtIdx],
        body: &Body,
    ) {
        let header = self.cfg.add_vertex_with_origin(header, StmtId::from_idx(stmt_id));

        if let Some(current) = self.current_block {
            self.link_to(current, header);
        }

        let body_block = self.new_block();
        self.cfg.add_edge(header, body_block, CfgEdgeType::TrueBranch);

        let after_loop = self.new_block();
        self.loop_stack.push(LoopFrame { header, exit: after_loop });

        self.current_block = Some(body_block);

        for &loop_stmt_id in loop_body.iter() {
            self.walk_statement_hir(loop_stmt_id, body);
        }

        self.loop_stack.pop();

        if let Some(exit) = self.current_block {
            if self.is_block_reachable(exit) {
                self.cfg.add_edge(exit, header, CfgEdgeType::Unconditional);
            }
        }

        self.cfg.add_edge(header, after_loop, CfgEdgeType::FalseBranch);

        self.current_block = Some(after_loop);
    }

    /// The handler is taken as reachable from the entry of `Попытка`: an error
    /// may interrupt the protected statements before any of them completes.
    fn walk_try_statement_hir(&mut self, stmt_id: StmtIdx, body: &Body) {
        if let Stmt::Try { body: try_body, except } = body.stmt_idx(stmt_id) {
            let try_vertex =
                self.cfg.add_vertex_with_origin(CfgVertex::Try, StmtId::from_idx(stmt_id));

            if let Some(current) = self.current_block {
                self.cfg.add_edge(current, try_vertex, CfgEdgeType::Unconditional);
            }

            let try_block = self.new_block();
            self.cfg.add_edge(try_vertex, try_block, CfgEdgeType::Unconditional);

            let except_block = self.new_block();
            self.cfg.add_edge(try_vertex, except_block, CfgEdgeType::Exception);

            self.except_stack.push(except_block);

            self.current_block = Some(try_block);
            for &try_stmt_id in try_body.iter() {
                self.walk_statement_hir(try_stmt_id, body);
            }

            let try_exit = self.current_block;

            self.except_stack.pop();

            self.current_block = Some(except_block);
            for &except_stmt_id in except.iter() {
                self.walk_statement_hir(except_stmt_id, body);
            }

            let except_exit = self.current_block;

            let merge_block = self.new_block();

            if let Some(exit) = try_exit {
                self.link_to(exit, merge_block);
            }
            if let Some(exit) = except_exit {
                self.link_to(exit, merge_block);
            }

            self.current_block = Some(merge_block);
        }
    }
}

impl Default for CfgBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let builder = CfgBuilder::new();
        assert!(builder.current_block.is_none());
    }

    #[test]
    fn test_hir_based_cfg_simple() {
        use hir_def::{Binding, Body, Expr, Literal, Stmt};
        use ordered_float::NotNan;

        let mut body = Body::default();

        let var_a = body.bindings_mut().alloc(Binding::var(hir_def::Name::new("А")));

        let lit_42 =
            body.exprs_mut().alloc(Expr::Literal(Literal::Number(NotNan::new(42.0).unwrap())));
        let path_a = body.exprs_mut().alloc(Expr::Path(hir_def::Name::new("А")));

        let var_decl = body.stmts_mut().alloc(Stmt::VarDecl { bindings: vec![var_a].into() });
        let assign = body.stmts_mut().alloc(Stmt::Assign { target: path_a, value: lit_42 });
        let return_stmt = body.stmts_mut().alloc(Stmt::Return { value: Some(path_a) });

        body.set_body_stmts(vec![var_decl, assign, return_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        assert!(cfg.start().is_some(), "CFG should have a start");
        assert!(cfg.exit() != cfg.start().unwrap(), "Exit should differ from entry");

        let vertex_count = cfg.graph().node_count();
        assert!(
            vertex_count >= 2,
            "Should have at least entry and exit vertices, got {}",
            vertex_count
        );

        let exit = cfg.exit();
        let incoming_to_exit: Vec<_> = cfg.incoming_edges(exit).collect();
        assert!(!incoming_to_exit.is_empty(), "Exit should have incoming edges");
    }

    #[test]
    fn test_hir_based_cfg_if_statement() {
        use hir_def::{Body, Expr, Literal, Stmt};
        use ordered_float::NotNan;

        let mut body = Body::default();

        let true_lit = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let lit_1 =
            body.exprs_mut().alloc(Expr::Literal(Literal::Number(NotNan::new(1.0).unwrap())));
        let lit_2 =
            body.exprs_mut().alloc(Expr::Literal(Literal::Number(NotNan::new(2.0).unwrap())));

        let return_1 = body.stmts_mut().alloc(Stmt::Return { value: Some(lit_1) });
        let return_2 = body.stmts_mut().alloc(Stmt::Return { value: Some(lit_2) });

        let if_stmt = body.stmts_mut().alloc(Stmt::If(Box::new(hir_def::IfStmt {
            condition: true_lit,
            then_branch: vec![return_1].into(),
            elsif_branches: vec![].into(),
            else_branch: Some(vec![return_2].into()),
        })));

        body.set_body_stmts(vec![if_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let vertex_count = cfg.graph().node_count();
        assert!(
            vertex_count >= 5,
            "If-else CFG should have multiple vertices, got {}",
            vertex_count
        );

        let has_conditional = cfg.graph().node_indices().any(|idx| {
            if let Some(vertex) = cfg.vertex(idx) {
                matches!(vertex, CfgVertex::Conditional(_))
            } else {
                false
            }
        });
        assert!(has_conditional, "CFG should contain conditional vertex for if statement");
    }

    fn edges_of_kind(cfg: &ControlFlowGraph, kind: CfgEdgeType) -> Vec<(NodeIndex, NodeIndex)> {
        cfg.graph()
            .edge_indices()
            .filter_map(|e| {
                let (src, dst) = cfg.graph().edge_endpoints(e)?;
                let edge_kind = *cfg.graph().edge_weight(e)?;
                (edge_kind == kind).then_some((src, dst))
            })
            .collect()
    }

    fn vertex_is(
        cfg: &ControlFlowGraph,
        idx: NodeIndex,
        predicate: impl Fn(&CfgVertex) -> bool,
    ) -> bool {
        cfg.vertex(idx).is_some_and(predicate)
    }

    fn block_ends_with(
        cfg: &ControlFlowGraph,
        body: &Body,
        idx: NodeIndex,
        predicate: impl Fn(&Stmt) -> bool,
    ) -> bool {
        matches!(
            cfg.vertex(idx),
            Some(CfgVertex::BasicBlock(bb)) if bb.last_statement().is_some_and(|s| predicate(body.stmt(s)))
        )
    }

    fn label_block_of(cfg: &ControlFlowGraph, label: StmtIdx) -> NodeIndex {
        cfg.vertices()
            .find(|(_, v)| {
                matches!(v, CfgVertex::BasicBlock(bb) if bb.first_statement() == Some(StmtId::from_idx(label)))
            })
            .map(|(idx, _)| idx)
            .expect("the label must head a basic block")
    }

    fn source_stmt_id_of_kind(
        cfg: &ControlFlowGraph,
        predicate: impl Fn(&CfgVertex) -> bool,
    ) -> Option<StmtId> {
        cfg.vertices()
            .find(|(_, vertex)| predicate(vertex))
            .and_then(|(idx, _)| cfg.source_stmt_id(idx))
    }

    #[test]
    fn conditional_vertex_exposes_originating_statement_id() {
        use hir_def::{Body, Expr, IfStmt, Literal, Stmt};

        let mut body = Body::default();
        let condition = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let if_stmt = body.stmts_mut().alloc(Stmt::If(Box::new(IfStmt {
            condition,
            then_branch: Box::default(),
            elsif_branches: Box::default(),
            else_branch: None,
        })));
        body.set_body_stmts(vec![if_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        assert_eq!(
            source_stmt_id_of_kind(&cfg, |vertex| matches!(vertex, CfgVertex::Conditional(_))),
            Some(StmtId::from_idx(if_stmt))
        );
    }

    #[test]
    fn while_loop_vertex_exposes_originating_statement_id() {
        use hir_def::{Body, Expr, Literal, Stmt};

        let mut body = Body::default();
        let condition = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let while_stmt = body.stmts_mut().alloc(Stmt::While { condition, body: Box::default() });
        body.set_body_stmts(vec![while_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        assert_eq!(
            source_stmt_id_of_kind(&cfg, |vertex| matches!(vertex, CfgVertex::WhileHeader(_))),
            Some(StmtId::from_idx(while_stmt))
        );
    }

    #[test]
    fn try_except_vertex_exposes_originating_statement_id() {
        use hir_def::{Body, Stmt};

        let mut body = Body::default();
        let try_stmt =
            body.stmts_mut().alloc(Stmt::Try { body: Box::default(), except: Box::default() });
        body.set_body_stmts(vec![try_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        assert_eq!(
            source_stmt_id_of_kind(&cfg, |vertex| matches!(vertex, CfgVertex::Try)),
            Some(StmtId::from_idx(try_stmt))
        );
    }

    #[test]
    fn label_starts_a_block_it_heads() {
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let label_stmt = body.stmts_mut().alloc(Stmt::Label(Name::new("Метка")));
        body.set_body_stmts(vec![label_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let label_block = label_block_of(&cfg, label_stmt);
        assert_ne!(Some(label_block), cfg.start(), "a label opens a block of its own");
    }

    #[test]
    fn break_in_while_jumps_to_after_loop() {
        use hir_def::{Body, Expr, Literal, Stmt};

        let mut body = Body::default();
        let true_lit = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let break_stmt = body.stmts_mut().alloc(Stmt::Break);
        let while_stmt = body
            .stmts_mut()
            .alloc(Stmt::While { condition: true_lit, body: vec![break_stmt].into() });
        body.set_body_stmts(vec![while_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let while_vertex = cfg
            .graph()
            .node_indices()
            .find(|&idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::WhileHeader(_))))
            .expect("WhileHeader vertex must exist");
        let after_loop = cfg
            .outgoing_edges(while_vertex)
            .find(|(_, e)| **e == CfgEdgeType::FalseBranch)
            .map(|(target, _)| target)
            .expect("WhileHeader must have a FalseBranch successor");

        let breaks: Vec<_> = edges_of_kind(&cfg, CfgEdgeType::Unconditional)
            .into_iter()
            .filter(|(src, _)| block_ends_with(&cfg, &body, *src, |s| matches!(s, Stmt::Break)))
            .collect();
        assert!(!breaks.is_empty(), "jump edge missing for `Прервать`");
        assert!(
            breaks.iter().any(|(_, dst)| *dst == after_loop),
            "`Прервать` must target the after-loop merge block, got {breaks:?}",
        );
    }

    #[test]
    fn continue_in_while_jumps_to_header() {
        use hir_def::{Body, Expr, Literal, Stmt};

        let mut body = Body::default();
        let true_lit = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let cont_stmt = body.stmts_mut().alloc(Stmt::Continue);
        let while_stmt = body
            .stmts_mut()
            .alloc(Stmt::While { condition: true_lit, body: vec![cont_stmt].into() });
        body.set_body_stmts(vec![while_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let while_vertex = cfg
            .graph()
            .node_indices()
            .find(|&idx| vertex_is(&cfg, idx, |v| matches!(v, CfgVertex::WhileHeader(_))))
            .expect("WhileHeader vertex must exist");

        let continues: Vec<_> = edges_of_kind(&cfg, CfgEdgeType::Unconditional)
            .into_iter()
            .filter(|(src, _)| block_ends_with(&cfg, &body, *src, |s| matches!(s, Stmt::Continue)))
            .collect();
        assert!(!continues.is_empty(), "jump edge missing for `Продолжить`");
        assert!(
            continues.iter().any(|(_, dst)| *dst == while_vertex),
            "`Продолжить` must target the loop header, got {continues:?}",
        );
    }

    #[test]
    fn break_outside_loop_emits_no_executable_edge() {
        use hir_def::{Body, Stmt};

        let mut body = Body::default();
        let break_stmt = body.stmts_mut().alloc(Stmt::Break);
        body.set_body_stmts(vec![break_stmt].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let start = cfg.start().expect("start block");
        assert!(
            cfg.outgoing_edges(start).all(|(_, kind)| !kind.is_executable()),
            "Bare `Прервать` outside a loop must not produce an executable edge",
        );
    }

    #[test]
    fn goto_backward_resolves_to_existing_label() {
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let label = body.stmts_mut().alloc(Stmt::Label(Name::new("М")));
        let goto = body.stmts_mut().alloc(Stmt::Goto(Name::new("М")));
        body.set_body_stmts(vec![label, goto].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let label_block = label_block_of(&cfg, label);
        let jumps: Vec<_> = edges_of_kind(&cfg, CfgEdgeType::Unconditional)
            .into_iter()
            .filter(|(_, dst)| *dst == label_block)
            .collect();
        assert!(
            jumps.len() >= 2,
            "Backward `Перейти` must add an edge to the existing label block, got {jumps:?}",
        );
    }

    #[test]
    fn goto_forward_resolves_when_label_arrives() {
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let goto = body.stmts_mut().alloc(Stmt::Goto(Name::new("М")));
        let label = body.stmts_mut().alloc(Stmt::Label(Name::new("М")));
        body.set_body_stmts(vec![goto, label].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let label_block = label_block_of(&cfg, label);
        let jump_from_goto =
            edges_of_kind(&cfg, CfgEdgeType::Unconditional).into_iter().any(|(src, dst)| {
                dst == label_block
                    && block_ends_with(&cfg, &body, src, |s| matches!(s, Stmt::Goto(_)))
            });
        assert!(
            jump_from_goto,
            "Forward `Перейти` must be patched with an edge once the label arrives",
        );
    }

    #[test]
    fn unresolved_goto_leaves_no_executable_edge() {
        use hir_def::{Body, Name, Stmt};

        let mut body = Body::default();
        let goto = body.stmts_mut().alloc(Stmt::Goto(Name::new("Нет")));
        body.set_body_stmts(vec![goto].into());

        let cfg = CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), &body);

        let start = cfg.start().expect("start block");
        assert!(
            cfg.outgoing_edges(start).all(|(_, kind)| !kind.is_executable()),
            "Unresolved goto must NOT fabricate a jump target",
        );
    }
}
