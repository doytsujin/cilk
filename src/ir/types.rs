use super::value::Value;
use id_arena::*;
use rustc_hash::FxHashMap;
use std::convert::From;
use std::fmt;
use std::{cell::Ref, cell::RefCell, rc::Rc};

#[derive(Clone)]
pub struct Types {
    pub base: Rc<RefCell<TypesBase>>,
}

#[derive(Clone)]
pub struct TypesBase {
    pub compound_types: Arena<CompoundType>,
}

pub type CompoundTypeId = Id<CompoundType>;

#[derive(Clone, Eq, PartialEq)]
pub enum CompoundType {
    Pointer(Type),
    Array(ArrayType),
    Function(FunctionType),
    Struct(StructType),
}

#[allow(non_camel_case_types)]
#[derive(Clone, PartialEq, Eq, Copy, Hash)]
pub enum Type {
    Void,
    i1,
    i8,
    i32,
    i64,
    f64,
    Pointer(CompoundTypeId),
    Array(CompoundTypeId),
    Function(CompoundTypeId),
    Struct(CompoundTypeId),
}

pub trait TypeSize {
    fn size_in_byte(&self, tys: &Types) -> usize;
    fn size_in_bits(&self, tys: &Types) -> usize;
    fn align_in_byte(&self, tys: &Types) -> usize;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionType {
    pub ret_ty: Type,
    pub params_ty: Vec<Type>,
    pub params_attr: FxHashMap<usize, ParamAttribute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ParamAttribute {
    pub byval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrayType {
    pub elem_ty: Type,
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructType {
    fields_ty: Vec<Type>,
    fields_offset: Vec<usize>,
    align: usize,
    size: usize,
}

impl Types {
    pub fn new() -> Self {
        Self {
            base: Rc::new(RefCell::new(TypesBase::new())),
            // compound_types: Arena::new(),
        }
    }

    fn new_compound_ty(&self, t: CompoundType) -> CompoundTypeId {
        let compound_types = &mut self.base.borrow_mut().compound_types;
        for (id, t_) in &*compound_types {
            if &t == t_ {
                return id;
            }
        }
        compound_types.alloc(t)
    }

    pub fn new_pointer_ty(&self, elem_ty: Type) -> Type {
        let id = self.new_compound_ty(CompoundType::Pointer(elem_ty));
        Type::Pointer(id)
    }

    pub fn new_array_ty(&self, elem_ty: Type, len: usize) -> Type {
        let id = self.new_compound_ty(CompoundType::Array(ArrayType::new(elem_ty, len)));
        Type::Array(id)
    }

    pub fn new_function_ty(&self, ret_ty: Type, mut params_ty: Vec<Type>) -> Type {
        let mut params_attr = FxHashMap::default();
        for (i, ty) in params_ty.iter_mut().enumerate() {
            match ty {
                Type::Struct(_) => {
                    let ptr = self.new_pointer_ty(*ty);
                    *ty = ptr;
                    params_attr.insert(i, ParamAttribute { byval: true });
                }
                _ => {}
            }
        }
        let id = self.new_compound_ty(CompoundType::Function(FunctionType::new(
            ret_ty,
            params_ty,
            params_attr,
        )));
        Type::Function(id)
    }

    pub fn new_struct_ty(&self, fields_ty: Vec<Type>) -> Type {
        let id = self.new_compound_ty(CompoundType::Struct(StructType::new(self, fields_ty)));
        Type::Struct(id)
    }

    pub fn compound_ty<T: Into<CompoundTypeId>>(&self, id: T) -> Ref<CompoundType> {
        Ref::map(self.base.borrow(), |x| &x.compound_types[id.into()])
    }

    pub fn get_element_ty(&self, ty: Type, index: Option<&Value>) -> Option<Type> {
        match ty {
            Type::Pointer(id) => Some(*self.base.borrow().compound_types[id].as_pointer()),
            Type::Array(id) => Some(self.base.borrow().compound_types[id].as_array().elem_ty),
            Type::Struct(id) => Some(
                self.base.borrow().compound_types[id].as_struct().fields_ty
                    [index.unwrap().as_imm().as_int32() as usize],
            ),
            Type::Void
            | Type::i1
            | Type::i8
            | Type::i32
            | Type::i64
            | Type::f64
            | Type::Function(_) => Some(ty),
        }
    }

    pub fn get_element_ty_with_indices(&self, ty: Type, indices: &[Value]) -> Option<Type> {
        if indices.len() == 0 {
            return Some(ty);
        }

        match ty {
            Type::Void
            | Type::i1
            | Type::i8
            | Type::i32
            | Type::i64
            | Type::f64
            | Type::Function(_) => None,
            Type::Pointer(id) => match indices.len() {
                1 => Some(*self.base.borrow().compound_types[id].as_pointer()),
                _ => {
                    let elem_ty = *self.base.borrow().compound_types[id].as_pointer();
                    self.get_element_ty_with_indices(elem_ty, &indices[1..])
                }
            },
            Type::Array(id) => match indices.len() {
                1 => Some(self.base.borrow().compound_types[id].as_array().elem_ty),
                _ => {
                    let elem_ty = self.base.borrow().compound_types[id].as_array().elem_ty;
                    self.get_element_ty_with_indices(elem_ty, &indices[1..])
                }
            },
            Type::Struct(id) => match indices.len() {
                1 => Some(
                    self.base.borrow().compound_types[id].as_struct().fields_ty
                        [indices[0].as_imm().as_int32() as usize],
                ),
                _ => self.get_element_ty_with_indices(
                    self.base.borrow().compound_types[id].as_struct().fields_ty
                        [indices[0].as_imm().as_int32() as usize],
                    &indices[1..],
                ),
            },
        }
    }

    pub fn to_string(&self, ty: Type) -> String {
        self.base.borrow().to_string(ty)
    }

    // pub fn get_pointer_ty(&self) -> Type {
    //     Type::Pointer(Box::new(self.clone()))
    // }
}

impl TypesBase {
    pub fn new() -> Self {
        Self {
            compound_types: Arena::new(),
        }
    }

    fn new_compound_ty(&mut self, t: CompoundType) -> CompoundTypeId {
        for (id, t_) in &self.compound_types {
            if &t == t_ {
                return id;
            }
        }
        self.compound_types.alloc(t)
    }

    pub fn new_pointer_ty(&mut self, elem_ty: Type) -> Type {
        let id = self.new_compound_ty(CompoundType::Pointer(elem_ty));
        Type::Pointer(id)
    }

    pub fn new_array_ty(&mut self, elem_ty: Type, len: usize) -> Type {
        let id = self.new_compound_ty(CompoundType::Array(ArrayType::new(elem_ty, len)));
        Type::Array(id)
    }

    pub fn new_function_ty(&mut self, ret_ty: Type, mut params_ty: Vec<Type>) -> Type {
        let mut params_attr = FxHashMap::default();
        for (i, ty) in params_ty.iter_mut().enumerate() {
            match ty {
                Type::Struct(_) => {
                    let ptr = self.new_pointer_ty(*ty);
                    *ty = ptr;
                    params_attr.insert(i, ParamAttribute { byval: true });
                }
                _ => {}
            }
        }
        let id = self.new_compound_ty(CompoundType::Function(FunctionType::new(
            ret_ty,
            params_ty,
            params_attr,
        )));
        Type::Function(id)
    }

    // pub fn new_struct_ty(&mut self, fields_ty: Vec<Type>) -> Type {
    //     let id = self.new_compound_ty(CompoundType::Struct(StructType::new(fields_ty)));
    //     Type::Struct(id)
    // }

    pub fn as_function_ty(&self, ty: Type) -> Option<&FunctionType> {
        match ty {
            Type::Function(id) => Some(self.compound_types[id].as_function()),
            _ => None,
        }
    }

    pub fn as_struct_ty(&self, ty: Type) -> Option<&StructType> {
        match ty {
            Type::Struct(id) => Some(self.compound_types[id].as_struct()),
            _ => None,
        }
    }

    pub fn get_element_ty(&self, ty: Type, index: Option<&Value>) -> Option<Type> {
        match ty {
            Type::Pointer(id) => Some(*self.compound_types[id].as_pointer()),
            Type::Array(id) => Some(self.compound_types[id].as_array().elem_ty),
            Type::Struct(id) => Some(
                self.compound_types[id].as_struct().fields_ty
                    [index.unwrap().as_imm().as_int32() as usize],
            ),
            Type::Void
            | Type::i1
            | Type::i8
            | Type::i32
            | Type::i64
            | Type::f64
            | Type::Function(_) => Some(ty),
        }
    }

    pub fn get_element_ty_with_indices(&self, ty: Type, indices: &[Value]) -> Option<Type> {
        if indices.len() == 0 {
            return Some(ty);
        }

        match ty {
            Type::Void
            | Type::i1
            | Type::i8
            | Type::i32
            | Type::i64
            | Type::f64
            | Type::Function(_) => None,
            Type::Pointer(id) => match indices.len() {
                1 => Some(*self.compound_types[id].as_pointer()),
                _ => {
                    let elem_ty = self.compound_types[id].as_pointer();
                    self.get_element_ty_with_indices(*elem_ty, &indices[1..])
                }
            },
            Type::Array(id) => match indices.len() {
                1 => Some(self.compound_types[id].as_array().elem_ty),
                _ => {
                    let elem_ty = self.compound_types[id].as_array().elem_ty;
                    self.get_element_ty_with_indices(elem_ty, &indices[1..])
                }
            },
            Type::Struct(id) => match indices.len() {
                1 => Some(
                    self.compound_types[id].as_struct().fields_ty
                        [indices[0].as_imm().as_int32() as usize],
                ),
                _ => self.get_element_ty_with_indices(
                    self.compound_types[id].as_struct().fields_ty
                        [indices[0].as_imm().as_int32() as usize],
                    &indices[1..],
                ),
            },
        }
    }

    pub fn to_string(&self, ty: Type) -> String {
        match ty {
            Type::Void => "void".to_string(),
            Type::i1 => "i1".to_string(),
            Type::i8 => "i8".to_string(),
            Type::i32 => "i32".to_string(),
            Type::i64 => "i64".to_string(),
            Type::f64 => "f64".to_string(),
            Type::Pointer(id) => {
                let elem_ty = self.compound_types[id].as_pointer();
                format!("{}*", self.to_string(*elem_ty))
            }
            Type::Array(id) => {
                let arr = self.compound_types[id].as_array();
                arr.to_string(self)
            }
            Type::Function(id) => {
                let f = self.compound_types[id].as_function();
                f.to_string(self)
            }
            Type::Struct(id) => {
                let s = self.compound_types[id].as_struct();
                s.to_string(self)
            }
        }
    }

    // pub fn get_pointer_ty(&self) -> Type {
    //     Type::Pointer(Box::new(self.clone()))
    // }
}

impl Type {
    pub fn is_atomic(&self) -> bool {
        matches!(
            self,
            Self::Void | Self::i1 | Self::i8 | Self::i32 | Self::i64 | Self::f64
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Self::i1 | Self::i8 | Self::i32 | Self::i64)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Self::f64)
    }

    pub fn to_string(&self) -> String {
        match self {
            Type::Void => "void".to_string(),
            Type::i1 => "i1".to_string(),
            Type::i8 => "i8".to_string(),
            Type::i32 => "i32".to_string(),
            Type::i64 => "i64".to_string(),
            Type::f64 => "f64".to_string(),
            Type::Pointer(id) => format!("(ty:{})*", id.index()),
            Type::Array(id) => format!("arrty:{}", id.index()),
            Type::Function(id) => format!("functy:{}", id.index()),
            Type::Struct(id) => format!("structty:{}", id.index()),
        }
    }
}

impl FunctionType {
    pub fn new(
        ret_ty: Type,
        params_ty: Vec<Type>,
        params_attr: FxHashMap<usize, ParamAttribute>,
    ) -> Self {
        Self {
            ret_ty,
            params_ty,
            params_attr,
        }
    }

    pub fn to_string(&self, tys: &TypesBase) -> String {
        format!(
            "{} ({})",
            tys.to_string(self.ret_ty),
            self.params_ty
                .iter()
                .enumerate()
                .fold("".to_string(), |mut s, (i, p)| {
                    s += &(tys.to_string(*p)
                        + self
                            .params_attr
                            .get(&i)
                            .map_or("", |a| if a.byval { " byval" } else { "" })
                        + ", ");
                    s
                })
                .trim_matches(&[',', ' '][0..]),
        )
    }
}

impl ArrayType {
    pub fn new(elem_ty: Type, len: usize) -> Self {
        Self { elem_ty, len }
    }

    pub fn to_string(&self, tys: &TypesBase) -> String {
        format!("[{} x {}]", self.len, tys.to_string(self.elem_ty))
    }
}

impl StructType {
    pub fn new(tys: &Types, fields_ty: Vec<Type>) -> Self {
        let mut self_ = Self {
            fields_ty,
            fields_offset: vec![],
            align: 0,
            size: 0,
        };
        self_.compute_elem_offsets(tys);
        self_
    }

    pub fn compute_elem_offsets(&mut self, tys: &Types) {
        let mut align = 1;
        let mut offset = 0;
        let padding = |off, align| -> usize { (align - off % align) % align };
        for ty in &self.fields_ty {
            align = ::std::cmp::max(ty.align_in_byte(tys), align);
            let size = ty.size_in_byte(tys);
            let align = ty.align_in_byte(tys);
            offset += padding(offset, align);
            self.fields_offset.push(offset);
            offset += size;
        }
        self.size = offset + padding(offset, align);
        self.align = align;
    }

    pub const fn size(&self) -> usize {
        self.size
    }

    pub const fn align(&self) -> usize {
        self.align
    }

    pub fn get_elem_offset(&self, i: usize) -> Option<&usize> {
        self.fields_offset.get(i)
    }

    pub fn get_type_at(&self, i: usize) -> Option<&Type> {
        self.fields_offset
            .iter()
            .position(|&off| off == i)
            .map_or(None, |n| Some(&self.fields_ty[n]))
    }

    pub fn to_string(&self, tys: &TypesBase) -> String {
        format!(
            "struct {{{}}}",
            self.fields_ty
                .iter()
                .fold("".to_string(), |mut s, t| {
                    s += &(tys.to_string(*t) + ", ");
                    s
                })
                .trim_matches(&[',', ' '][0..])
        )
    }
}

impl CompoundType {
    pub fn as_pointer(&self) -> &Type {
        match self {
            CompoundType::Pointer(p) => p,
            _ => panic!(),
        }
    }

    pub fn as_array(&self) -> &ArrayType {
        match self {
            CompoundType::Array(a) => a,
            _ => panic!(),
        }
    }

    pub fn as_function(&self) -> &FunctionType {
        match self {
            CompoundType::Function(f) => f,
            _ => panic!(),
        }
    }

    pub fn as_struct(&self) -> &StructType {
        match self {
            CompoundType::Struct(s) => s,
            _ => panic!(),
        }
    }
}

impl From<Type> for CompoundTypeId {
    fn from(x: Type) -> CompoundTypeId {
        match x {
            Type::Pointer(id) | Type::Array(id) | Type::Function(id) | Type::Struct(id) => id,
            _ => panic!(),
        }
    }
}

impl fmt::Debug for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl fmt::Debug for Types {
    fn fmt(&self, _: &mut fmt::Formatter) -> fmt::Result {
        fmt::Result::Ok(())
    }
}
