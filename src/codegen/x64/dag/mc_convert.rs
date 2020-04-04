// TODO: dirty code

use super::super::inst::DefOrUseReg;
use super::super::machine::{basic_block::*, function::*, inst::*, module::*};
use super::super::register::*;
use super::{basic_block::*, function::*, module::*, node::*};
use crate::ir::types::*;
use crate::util::allocator::*;
use rustc_hash::FxHashMap;
use std::cell::RefCell;

pub struct MIConverter {
    pub dag_bb_to_machine_bb: FxHashMap<DAGBasicBlockId, MachineBasicBlockId>,
    pub node_id_to_machine_inst_id: FxHashMap<Raw<DAGNode>, MachineInstId>,
}

pub struct ConversionInfo<'a> {
    cur_func: &'a DAGFunction,
    machine_inst_arena: &'a mut InstructionArena,
    dag_to_reg: FxHashMap<Raw<DAGNode>, Option<MachineRegister>>,
    cur_bb: MachineBasicBlockId,
    iseq: &'a mut Vec<MachineInstId>,
    stack_adjust: &'a mut usize,
}

impl MIConverter {
    pub fn new() -> Self {
        Self {
            dag_bb_to_machine_bb: FxHashMap::default(),
            node_id_to_machine_inst_id: FxHashMap::default(),
        }
    }

    pub fn convert_module(&mut self, module: DAGModule) -> MachineModule {
        let mut machine_module = MachineModule::new(module.name.as_str());
        for (_, func) in module.functions {
            machine_module.add_function(self.convert_function(&module.types, func));
        }
        machine_module.types = module.types.clone();
        machine_module
    }

    pub fn convert_function(&mut self, tys: &Types, mut dag_func: DAGFunction) -> MachineFunction {
        self.dag_bb_to_machine_bb.clear();

        let mut mbbs = MachineBasicBlocks::new();

        for dag_bb_id in &dag_func.dag_basic_blocks {
            let mbb_id = mbbs.arena.alloc(MachineBasicBlock::new());
            self.dag_bb_to_machine_bb.insert(*dag_bb_id, mbb_id);
            mbbs.order.push(mbb_id);
        }

        for (dag_bb, machine_bb) in &self.dag_bb_to_machine_bb {
            mbbs.arena[*machine_bb].pred = dag_func.dag_basic_block_arena[*dag_bb]
                .pred
                .iter()
                .map(|bb| self.get_machine_bb(*bb))
                .collect();
            mbbs.arena[*machine_bb].succ = dag_func.dag_basic_block_arena[*dag_bb]
                .succ
                .iter()
                .map(|bb| self.get_machine_bb(*bb))
                .collect();
        }

        let mut machine_inst_arena = InstructionArena::new();

        let mut stack_adjust = 0;
        for dag_bb_id in &dag_func.dag_basic_blocks {
            let node = &dag_func.dag_basic_block_arena[*dag_bb_id];
            let bb_id = self.get_machine_bb(*dag_bb_id);
            let mut iseq = vec![];

            self.convert_dag(
                tys,
                &mut ConversionInfo {
                    cur_func: &dag_func,
                    machine_inst_arena: &mut machine_inst_arena,
                    dag_to_reg: FxHashMap::default(),
                    cur_bb: bb_id,
                    iseq: &mut iseq,
                    stack_adjust: &mut stack_adjust,
                },
                node.entry.unwrap(),
            );

            mbbs.arena[bb_id].iseq = RefCell::new(iseq);
        }

        dag_func.local_mgr.adjust_byte = stack_adjust;

        MachineFunction::new(dag_func, mbbs, machine_inst_arena)
    }

    pub fn convert_dag(
        &mut self,
        tys: &Types,
        conv_info: &mut ConversionInfo,
        node: Raw<DAGNode>,
    ) -> Option<MachineRegister> {
        if let Some(machine_register) = conv_info.dag_to_reg.get(&node) {
            return machine_register.clone();
        }

        let machine_register = match self.convert_dag_to_machine_inst(tys, conv_info, node) {
            Some(id) => {
                let inst = &conv_info.machine_inst_arena[id];
                inst.get_def_reg().map(|r| r.clone())
            }
            None => None,
        };
        conv_info.dag_to_reg.insert(node, machine_register.clone());

        some_then!(next, node.next, {
            self.convert_dag(tys, conv_info, next);
        });

        machine_register
    }

    fn convert_dag_to_machine_inst(
        &mut self,
        tys: &Types,
        conv_info: &mut ConversionInfo,
        node: Raw<DAGNode>,
    ) -> Option<MachineInstId> {
        if let Some(machine_inst_id) = self.node_id_to_machine_inst_id.get(&node) {
            return Some(*machine_inst_id);
        }

        #[rustfmt::skip]
        macro_rules! bb {($id:expr)=>{ $id.as_basic_block() };}
        #[rustfmt::skip]
        macro_rules! cond_kind {($id:expr)=>{ $id.as_cond_kind() };}

        // there should be no NodeKind::IRs here
        let machine_inst_id = match &node.kind {
            NodeKind::MI(_) => {
                fn reg(inst: &MachineInst, x: &DefOrUseReg) -> MachineRegister {
                    match x {
                        DefOrUseReg::Def(i) => inst.def[*i].clone(),
                        DefOrUseReg::Use(i) => inst.operand[*i].as_register().clone(),
                    }
                }
                let mi = node.kind.as_mi();
                let inst_def = mi.inst_def().unwrap();
                let operands = node
                    .operand
                    .iter()
                    .map(|op| self.normal_operand(tys, conv_info, *op))
                    .collect();
                let mut inst = MachineInst::new(
                    &conv_info.cur_func.vreg_gen,
                    mi,
                    operands,
                    ty2rc(&node.ty),
                    conv_info.cur_bb,
                );
                for (def_, use_) in &inst_def.tie {
                    inst.tie_regs(reg(&inst, def_), reg(&inst, use_));
                }
                Some(conv_info.push_inst(inst))
            }
            NodeKind::IR(IRNodeKind::Entry) => None,
            NodeKind::IR(IRNodeKind::CopyToReg) => {
                let val = self.normal_operand(tys, conv_info, node.operand[1]);
                let dst = match &node.operand[0].kind {
                    NodeKind::Operand(OperandNodeKind::Register(r)) => {
                        MachineRegister::new(r.clone())
                    }
                    _ => unreachable!(),
                };
                Some(conv_info.push_inst(MachineInst::new_with_def_reg(
                    MachineOpcode::Copy,
                    vec![val],
                    vec![dst],
                    conv_info.cur_bb,
                )))
            }
            NodeKind::IR(IRNodeKind::Call) => Some(self.convert_call_dag(tys, conv_info, &*node)),
            NodeKind::IR(IRNodeKind::Phi) => {
                let mut operands = vec![];
                let mut i = 0;
                while i < node.operand.len() {
                    operands.push(self.normal_operand(tys, conv_info, node.operand[i]));
                    operands.push(MachineOperand::Branch(
                        self.get_machine_bb(bb!(node.operand[i + 1])),
                    ));
                    i += 2;
                }
                Some(conv_info.push_inst(MachineInst::new(
                    &conv_info.cur_func.vreg_gen,
                    MachineOpcode::Phi,
                    operands,
                    ty2rc(&node.ty),
                    conv_info.cur_bb,
                )))
            }
            NodeKind::IR(IRNodeKind::Rem) => {
                let eax = RegisterInfo::new_phy_reg(GR32::EAX)
                    .with_vreg(conv_info.cur_func.vreg_gen.next_vreg())
                    .into_machine_register();
                let edx = RegisterInfo::new_phy_reg(GR32::EDX)
                    .with_vreg(conv_info.cur_func.vreg_gen.next_vreg())
                    .into_machine_register();

                let op1 = self.normal_operand(tys, conv_info, node.operand[0]);
                let op2 = self.normal_operand(tys, conv_info, node.operand[1]);

                conv_info.push_inst(
                    MachineInst::new_simple(
                        mov_n_rx(32, &op1).unwrap(),
                        vec![op1],
                        conv_info.cur_bb,
                    )
                    .with_def(vec![eax.clone()]),
                );

                conv_info.push_inst(
                    MachineInst::new_simple(MachineOpcode::CDQ, vec![], conv_info.cur_bb)
                        .with_imp_def(edx.clone())
                        .with_imp_use(eax.clone()),
                );

                assert_eq!(op2.get_type(), Some(Type::Int32));
                let inst1 = MachineInst::new(
                    &conv_info.cur_func.vreg_gen,
                    mov_n_rx(32, &op2).unwrap(),
                    vec![op2],
                    Some(RegisterClassKind::GR32), // TODO: support other types
                    conv_info.cur_bb,
                );
                let op2 = MachineOperand::Register(inst1.def[0].clone());
                conv_info.push_inst(inst1);

                conv_info.push_inst(
                    MachineInst::new_simple(MachineOpcode::IDIV, vec![op2], conv_info.cur_bb)
                        .with_imp_defs(vec![eax.clone(), edx.clone()])
                        .with_imp_uses(vec![eax, edx.clone()]),
                );

                Some(conv_info.push_inst(MachineInst::new(
                    &conv_info.cur_func.vreg_gen,
                    MachineOpcode::Copy,
                    vec![MachineOperand::Register(edx)],
                    Some(RegisterClassKind::GR32), // TODO
                    conv_info.cur_bb,
                )))
            }
            NodeKind::IR(IRNodeKind::Setcc) => {
                let new_op1 = self.normal_operand(tys, conv_info, node.operand[1]);
                let new_op2 = self.normal_operand(tys, conv_info, node.operand[2]);
                Some(conv_info.push_inst(MachineInst::new(
                    &conv_info.cur_func.vreg_gen,
                    match cond_kind!(node.operand[0]) {
                        CondKind::Eq => MachineOpcode::Seteq,
                        CondKind::Le => MachineOpcode::Setle,
                        CondKind::Lt => MachineOpcode::Setlt,
                    },
                    vec![new_op1, new_op2],
                    ty2rc(&node.ty),
                    conv_info.cur_bb,
                )))
            }
            NodeKind::IR(IRNodeKind::Br) => Some(conv_info.push_inst(MachineInst::new(
                &conv_info.cur_func.vreg_gen,
                MachineOpcode::JMP,
                vec![MachineOperand::Branch(
                    self.get_machine_bb(bb!(node.operand[0])),
                )],
                None,
                conv_info.cur_bb,
            ))),
            NodeKind::IR(IRNodeKind::BrCond) => {
                let new_cond = self.normal_operand(tys, conv_info, node.operand[0]);
                Some(conv_info.push_inst(MachineInst::new(
                    &conv_info.cur_func.vreg_gen,
                    MachineOpcode::BrCond,
                    vec![
                        new_cond,
                        MachineOperand::Branch(self.get_machine_bb(bb!(node.operand[1]))),
                    ],
                    None,
                    conv_info.cur_bb,
                )))
            }
            NodeKind::IR(IRNodeKind::Brcc) => {
                let op0 = self.normal_operand(tys, conv_info, node.operand[1]);
                let op1 = self.normal_operand(tys, conv_info, node.operand[2]);

                conv_info.push_inst(MachineInst::new_simple(
                    if op0.is_register() && op1.is_constant() {
                        MachineOpcode::CMPri
                    } else if op0.is_register() && op1.is_register() {
                        MachineOpcode::CMPrr
                    } else {
                        unreachable!()
                    },
                    vec![op0, op1],
                    conv_info.cur_bb,
                ));

                Some(conv_info.push_inst(MachineInst::new_simple(
                    match cond_kind!(node.operand[0]) {
                        CondKind::Eq => MachineOpcode::JE,
                        CondKind::Le => MachineOpcode::JLE,
                        CondKind::Lt => MachineOpcode::JL,
                    },
                    vec![MachineOperand::Branch(
                        self.get_machine_bb(bb!(node.operand[3])),
                    )],
                    conv_info.cur_bb,
                )))
            }
            NodeKind::IR(IRNodeKind::Ret) => Some(self.convert_ret(tys, conv_info, &*node)),
            NodeKind::IR(IRNodeKind::CopyToLiveOut) => {
                self.convert_dag_to_machine_inst(tys, conv_info, node.operand[0])
            }
            _ => None,
        };

        if machine_inst_id.is_some() {
            self.node_id_to_machine_inst_id
                .insert(node, machine_inst_id.unwrap());
        }

        machine_inst_id
    }

    fn convert_ret(
        &mut self,
        tys: &Types,
        conv_info: &mut ConversionInfo,
        node: &DAGNode,
    ) -> MachineInstId {
        let val = self.normal_operand(tys, conv_info, node.operand[0]);
        if let Some(ty) = val.get_type() {
            let ret_reg = ty2rc(&ty).unwrap().return_value_register();
            let set_ret_val =
                MachineInst::new_simple(mov_rx(tys, &val).unwrap(), vec![val], conv_info.cur_bb)
                    .with_def(vec![
                        RegisterInfo::new_phy_reg(ret_reg).into_machine_register()
                    ]);
            conv_info.push_inst(set_ret_val);
        }
        conv_info.push_inst(MachineInst::new_simple(
            MachineOpcode::RET,
            vec![],
            conv_info.cur_bb,
        ))
    }

    fn convert_call_dag(
        &mut self,
        tys: &Types,
        conv_info: &mut ConversionInfo,
        node: &DAGNode,
    ) -> MachineInstId {
        fn move_operand_to_reg(
            tys: &Types,
            op: MachineOperand,
            r: MachineRegister,
            bb: MachineBasicBlockId,
        ) -> MachineInst {
            match op {
                MachineOperand::Branch(_) => unimplemented!(),
                MachineOperand::Constant(_) | MachineOperand::Register(_) => {
                    MachineInst::new_simple(mov_rx(tys, &op).unwrap(), vec![op], bb)
                        .with_def(vec![r])
                }
                MachineOperand::FrameIndex(_) => {
                    let rbp = MachineOperand::Register(
                        RegisterInfo::new_phy_reg(GR64::RBP).into_machine_register(),
                    );
                    MachineInst::new_with_def_reg(
                        MachineOpcode::LEAr64m,
                        vec![rbp, op, MachineOperand::None, MachineOperand::None],
                        vec![r],
                        bb,
                    )
                }
                _ => unimplemented!(),
            }
        }

        let mut arg_regs = vec![];
        // TODO: We have to push remaining arguments on stack
        let mut off = 0;
        for (i, operand) in node.operand[1..].iter().enumerate() {
            let arg = self.normal_operand(tys, conv_info, *operand);
            let ty = arg.get_type().unwrap();

            // TODO: not simple
            let r = RegisterInfo::new(ty2rc(&ty).unwrap()); //.with_vreg(conv_info.cur_func.vreg_gen.next_vreg()); IS THIS REQUIRED?
            let inst = match r.reg_class.get_nth_arg_reg(i) {
                Some(arg_reg) => {
                    let r = r.with_reg(arg_reg).into_machine_register();
                    arg_regs.push(r.clone());
                    move_operand_to_reg(tys, arg, r, conv_info.cur_bb)
                }
                None => {
                    // conv_info.cur_func.local_mgr.alloc(&Type::Int32);
                    let rsp = MachineOperand::Register(
                        RegisterInfo::new_phy_reg(GR64::RSP).into_machine_register(),
                    );
                    let inst = MachineInst::new_simple(
                        mov_mx(&arg).unwrap(),
                        vec![
                            rsp,
                            MachineOperand::None,
                            MachineOperand::None,
                            MachineOperand::Constant(MachineConstant::Int32(off)),
                            arg,
                        ],
                        conv_info.cur_bb,
                    );
                    off += 8;
                    inst
                }
            };

            conv_info.push_inst(inst);
        }

        *conv_info.stack_adjust = ::std::cmp::max(*conv_info.stack_adjust, off as usize);

        let callee = self.normal_operand(tys, conv_info, node.operand[0]);

        let ret_reg = RegisterInfo::new_phy_reg(GR32::EAX)
            .with_vreg(conv_info.cur_func.vreg_gen.next_vreg())
            .into_machine_register(); // TODO: support types other than int.
                                      //       what about struct or union? they won't be assigned to register.

        let call_inst = conv_info.push_inst(
            MachineInst::new_with_imp_def_use(
                MachineOpcode::Call,
                vec![callee],
                if node.ty == Type::Void {
                    vec![]
                } else {
                    vec![ret_reg.clone()]
                },
                vec![], // TODO: imp-use
                conv_info.cur_bb,
            )
            .with_imp_uses(arg_regs),
        );

        if node.ty == Type::Void {
            call_inst
        } else {
            let reg_class = ret_reg.info_ref().reg_class;

            conv_info.push_inst(MachineInst::new(
                &conv_info.cur_func.vreg_gen,
                MachineOpcode::Copy,
                vec![MachineOperand::Register(ret_reg.clone())],
                Some(reg_class),
                conv_info.cur_bb,
            ))
        }
    }

    fn normal_operand(
        &mut self,
        tys: &Types,
        conv_info: &mut ConversionInfo,
        node: Raw<DAGNode>,
    ) -> MachineOperand {
        match node.kind {
            NodeKind::Operand(OperandNodeKind::Constant(c)) => match c {
                ConstantKind::Int32(i) => MachineOperand::Constant(MachineConstant::Int32(i)),
                ConstantKind::Int64(i) => MachineOperand::Constant(MachineConstant::Int64(i)),
                ConstantKind::F64(f) => MachineOperand::Constant(MachineConstant::F64(f)),
            },
            NodeKind::Operand(OperandNodeKind::FrameIndex(ref kind)) => {
                MachineOperand::FrameIndex(kind.clone())
            }
            NodeKind::Operand(OperandNodeKind::Address(ref g)) => match g {
                AddressKind::FunctionName(n) => {
                    MachineOperand::Address(AddressInfo::FunctionName(n.clone()))
                }
            },
            NodeKind::Operand(OperandNodeKind::BasicBlock(_)) => unimplemented!(),
            NodeKind::Operand(OperandNodeKind::Register(ref r)) => {
                MachineOperand::Register(MachineRegister::new(r.clone()))
            }
            NodeKind::None => MachineOperand::None,
            _ => MachineOperand::Register(self.convert_dag(tys, conv_info, node).unwrap()),
        }
    }

    fn get_machine_bb(&self, dag_bb_id: DAGBasicBlockId) -> MachineBasicBlockId {
        *self.dag_bb_to_machine_bb.get(&dag_bb_id).unwrap()
    }
}

impl<'a> ConversionInfo<'a> {
    pub fn push_inst(&mut self, inst: MachineInst) -> MachineInstId {
        let inst_id = self.machine_inst_arena.alloc(inst);
        self.iseq.push(inst_id);
        inst_id
    }
}

pub fn mov_n_rx(bit: usize, x: &MachineOperand) -> Option<MachineOpcode> {
    // TODO: refine code
    assert!(bit > 0 && ((bit & (bit - 1)) == 0));

    let mov32rx = [MachineOpcode::MOVrr32, MachineOpcode::MOVri32];
    let xidx = match x {
        MachineOperand::Register(_) => 0,
        MachineOperand::Constant(_) => 1,
        _ => return None, // TODO: Support Address?
    };
    match bit {
        32 => Some(mov32rx[xidx]),
        _ => None,
    }
}

pub fn mov_rx(tys: &Types, x: &MachineOperand) -> Option<MachineOpcode> {
    // TODO: special handling for float
    if x.get_type().unwrap() == Type::F64 {
        return Some(MachineOpcode::MOVSDrm64);
    }

    let mov32rx = [
        MachineOpcode::MOVrr32,
        MachineOpcode::MOVri32,
        MachineOpcode::MOVrm64,
    ];
    let mov64rx = [
        MachineOpcode::MOVrr64,
        MachineOpcode::MOVri64,
        MachineOpcode::MOVrm64,
    ];
    let (bit, xidx) = match x {
        MachineOperand::Register(r) => (r.info_ref().reg_class.size_in_bits(), 0),
        MachineOperand::Constant(c) => (c.size_in_bits(), 1),
        MachineOperand::FrameIndex(f) => (f.ty.size_in_bits(tys), 2),
        _ => return None, // TODO: Support Address?
    };
    match bit {
        32 => Some(mov32rx[xidx]),
        64 => Some(mov64rx[xidx]),
        _ => None,
    }
}

pub fn mov_mx(x: &MachineOperand) -> Option<MachineOpcode> {
    let mov32mx = [MachineOpcode::MOVmr32, MachineOpcode::MOVmi32];
    // let mov64rx = [
    //     MachineOpcode::MOVrr64,
    //     MachineOpcode::MOVri64,
    //     MachineOpcode::MOVrm64,
    // ];
    let (bit, n) = match x {
        MachineOperand::Register(r) => (r.info_ref().reg_class.size_in_bits(), 0),
        MachineOperand::Constant(c) => (c.size_in_bits(), 1),
        _ => return None, // TODO: Support Address?
    };
    match bit {
        32 => Some(mov32mx[n]),
        // 64 => Some(mov64rx[xidx]),
        _ => None,
    }
}
