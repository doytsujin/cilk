use super::{basic_block::*, function::*, instr::*, module::*};
use crate::ir::types::*;
// use super::{convert::*, node::*};
// use id_arena::*;
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct LiveRegMatrix {
    map: FxHashMap<usize, LiveInterval>,
}

#[derive(Debug, Clone)]
pub struct LiveInterval {
    vreg: usize,
    range: LiveRange,
}

#[derive(Debug, Clone)]
pub struct LiveRange {
    segments: Vec<LiveSegment>,
}

#[derive(Debug, Clone)]
pub struct LiveSegment {
    start: usize,
    end: usize,
}

impl LiveRegMatrix {
    pub fn new(map: FxHashMap<usize, LiveInterval>) -> Self {
        Self { map }
    }
}

impl LiveInterval {
    pub fn new(vreg: usize, range: LiveRange) -> Self {
        Self { vreg, range }
    }
}

impl LiveRange {
    pub fn new(segments: Vec<LiveSegment>) -> Self {
        Self { segments }
    }

    pub fn new_empty() -> Self {
        Self { segments: vec![] }
    }

    pub fn add_segment(&mut self, seg: LiveSegment) {
        self.segments.push(seg)
    }
}

impl LiveSegment {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn interference(&self, seg: &LiveSegment) -> bool {
        self.start < seg.end && self.end > seg.start
    }
}

pub struct LivenessAnalyzer<'a> {
    pub module: &'a MachineModule,
}

pub struct LivenessAnalysis<'a> {
    pub module: &'a MachineModule, // TODO: Will be used to get type
}

impl<'a> LivenessAnalysis<'a> {
    pub fn new(module: &'a MachineModule) -> Self {
        Self { module }
    }

    pub fn analyze_module(&mut self) {
        for (_, func) in &self.module.functions {
            self.analyze_function(func);
        }
    }

    pub fn analyze_function(&mut self, cur_func: &MachineFunction) -> LiveRegMatrix {
        self.set_def(cur_func);
        self.visit(cur_func);
        self.construct_live_reg_matrix(cur_func)
    }

    fn set_def(&mut self, cur_func: &MachineFunction) {
        for bb_id in &cur_func.basic_blocks {
            let bb = &cur_func.basic_block_arena[*bb_id];
            for instr_id in &*bb.iseq_ref() {
                self.set_def_on_instr(cur_func, bb, *instr_id);
            }
        }
    }

    fn set_def_on_instr(
        &mut self,
        cur_func: &MachineFunction,
        bb: &MachineBasicBlock,
        instr_id: MachineInstrId,
    ) {
        let instr = &cur_func.instr_arena[instr_id];

        if instr.def.len() > 0 {
            bb.liveness.borrow_mut().def.insert(instr.def[0].clone());
        }
    }

    fn visit(&mut self, cur_func: &MachineFunction) {
        for bb_id in &cur_func.basic_blocks {
            let bb = &cur_func.basic_block_arena[*bb_id];
            for instr_id in &*bb.iseq_ref() {
                self.visit_instr(cur_func, *bb_id, *instr_id);
            }
        }
    }

    fn visit_instr(
        &mut self,
        cur_func: &MachineFunction,
        bb: MachineBasicBlockId,
        instr_id: MachineInstrId,
    ) {
        let instr = &cur_func.instr_arena[instr_id];

        for (i, operand) in instr.operand.iter().enumerate() {
            if let MachineOperand::Register(reg) = operand {
                self.propagate(
                    cur_func,
                    bb,
                    if instr.opcode == MachineOpcode::Phi {
                        Some(instr.operand[i + 1].as_basic_block())
                    } else {
                        None
                    },
                    reg,
                )
            }
        }
    }

    fn propagate(
        &self,
        cur_func: &MachineFunction,
        bb: MachineBasicBlockId,
        pred_bb: Option<MachineBasicBlockId>,
        reg: &MachineRegister,
    ) {
        let bb = &cur_func.basic_block_arena[bb];

        {
            let mut bb_liveness = bb.liveness.borrow_mut();

            if bb_liveness.def.contains(reg) {
                return;
            }

            if !bb_liveness.live_in.insert(reg.clone()) {
                // live_in already had the reg
                return;
            }
        }

        if let Some(pred_bb) = pred_bb {
            let pred = &cur_func.basic_block_arena[pred_bb];
            if pred.liveness.borrow_mut().live_out.insert(reg.clone()) {
                // live_out didn't have the reg
                self.propagate(cur_func, pred_bb, None, reg);
            }
            return;
        }

        for pred_id in &bb.pred {
            let pred = &cur_func.basic_block_arena[*pred_id];
            if pred.liveness.borrow_mut().live_out.insert(reg.clone()) {
                // live_out didn't have the reg
                self.propagate(cur_func, *pred_id, None, reg);
            }
        }
    }

    pub fn construct_live_reg_matrix(&self, cur_func: &MachineFunction) -> LiveRegMatrix {
        let mut vreg2range = FxHashMap::default();

        let mut index = 0;

        for bb_id in &cur_func.basic_blocks {
            let bb = &cur_func.basic_block_arena[*bb_id];
            let liveness = bb.liveness_ref();

            let mut vreg2seg = FxHashMap::default();

            for livein in &liveness.live_in {
                vreg2seg.insert(livein.get_vreg(), LiveSegment::new(index, 0));
            }

            for instr_id in &*bb.iseq_ref() {
                let instr = &cur_func.instr_arena[*instr_id];

                if instr.def.len() > 0 {
                    vreg2seg.insert(instr.def[0].get_vreg(), LiveSegment::new(index, 0));
                }

                for operand in &instr.operand {
                    if let MachineOperand::Register(reg) = operand {
                        vreg2seg.get_mut(&reg.get_vreg()).unwrap().end = index;
                    }
                }

                index += 1;
            }

            for liveout in &liveness.live_out {
                vreg2seg.get_mut(&liveout.get_vreg()).unwrap().end = index;
            }

            for (vreg, seg) in vreg2seg {
                vreg2range
                    .entry(vreg)
                    .or_insert_with(|| LiveRange::new_empty())
                    .add_segment(seg)
            }
        }

        when_debug!(for (vreg, range) in &vreg2range {
            println!("vreg{}: {:?}", vreg, range)
        });

        LiveRegMatrix::new(
            vreg2range
                .into_iter()
                .map(|(vreg, range)| (vreg, LiveInterval::new(vreg, range)))
                .collect(),
        )
    }
}
