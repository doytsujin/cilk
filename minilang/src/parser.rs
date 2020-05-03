#[derive(Debug)]
pub struct Module {
    pub functions: Vec<Function>,
    pub structs: Vec<(String, Vec<(String, Type)>)>,
}

#[derive(Debug)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub ret_ty: Type,
    pub body: Vec<Node>,
}

#[derive(Debug)]
pub enum Node {
    Number(i32),
    Identifier(String),
    VarDecl(String, Type), // name, type
    Assign(String, Box<Node>),
    AssignIndex(String, Vec<Node>, Box<Node>),
    AssignDot(String, Vec<String>, Box<Node>),
    Eq(Box<Node>, Box<Node>),
    // Ne(Box<Node>, Box<Node>),
    Lt(Box<Node>, Box<Node>),
    Le(Box<Node>, Box<Node>),
    // Gt(Box<Node>, Box<Node>),
    // Ge(Box<Node>, Box<Node>),
    Add(Box<Node>, Box<Node>),
    Sub(Box<Node>, Box<Node>),
    Mul(Box<Node>, Box<Node>),
    Div(Box<Node>, Box<Node>),
    Rem(Box<Node>, Box<Node>),
    Index(String, Vec<Node>),
    Dot(String, Vec<String>),
    IfElse(Box<Node>, Vec<Node>, Option<Vec<Node>>),
    WhileLoop(Box<Node>, Vec<Node>),
    Call(String, Vec<Node>),
    Return(Box<Node>),
    Dot2(Box<Node>, Box<Node>),
    Index2(Box<Node>, Box<Node>),
    Assign2(Box<Node>, Box<Node>),
    Load(Box<Node>),
    // GlobalDataAddr(String),
}

#[derive(Debug, Clone)]
pub enum Type {
    Void,
    Int1,
    Int32,
    Int64,
    Pointer(Box<Type>),
    Array(usize, Box<Type>),
    Struct(String),
}

enum ModuleItem {
    Function(Function),
    StructDecl(String, Vec<(String, Type)>),
}

peg::parser!(pub grammar parser() for str {
    pub rule module() -> Module
        = items:((_ i:module_item() _ { i })*) {
            let mut functions = vec![];
            let mut structs = vec![];
            for item in items {
                match item {
                    ModuleItem::Function(f) => functions.push(f),
                    ModuleItem::StructDecl(name, decls) => structs.push((name, decls)),
                }
            }
            Module { functions, structs }
        }

    rule module_item() -> ModuleItem
        = f:function()    { ModuleItem::Function(f) }
        / s:struct_decl() { ModuleItem::StructDecl(s.0, s.1) }

    rule struct_decl() -> (String, Vec<(String, Type)>)
        = "struct" _ name:identifier() _
        "{" _
        d:((_ name:identifier() _ ":" _ ty:types() _ {(name, ty)}) ** ",") _
        "}" _
        { (name, d) }

    pub rule function() -> Function // (String, Vec<(String, String)>, String, Vec<Node>)
        = [' ' | '\t' | '\n']* "function" _ name:identifier() _
        "(" params:((_ i:identifier() _ ":" _ t:types() _ {(i, t)}) ** ",") ")" _
        ":" _
        ret_ty:types() _
        "{" _
        body:statements()
        _ "}" _
        { Function { name, params, ret_ty, body } }

    rule statements() -> Vec<Node>
        = s:((s:statement() _ {s})*) { s }

    rule statement() -> Node
        = var_decl()
        / if_else()
        / while_loop()
        / return_stmt()
        / assignment()
        / e:expression() _ ";" { e }

    rule expression() -> Node
        = binary_op()

    rule var_decl() -> Node
         = "var" _ name:identifier() _ ":" _ ty:types() _ ";" { Node::VarDecl(name, ty) }

    rule if_else() -> Node
        = "if" _ e:expression() _ "{" _
        then_body:statements() _ "}" _
        else_body:("else" _ "{" _ e:statements() _ "}" { e })?
        { Node::IfElse(Box::new(e), then_body, else_body) }

    rule while_loop() -> Node
        = "while" _ e:expression() _ "{" _
        loop_body:statements() _ "}"
        { Node::WhileLoop(Box::new(e), loop_body) }

    rule assignment() -> Node
        = p:primary() _ "=" _ e:expression() _ ";" {Node::Assign2(Box::new(p), Box::new(e))}
        // / i:index() _ "=" _ e:expression() _ ";" { Node::AssignIndex(i.0, i.1, Box::new(e))}
        // / i:dot() _ "=" _ e:expression() _ ";" { Node::AssignDot(i.0, i.1, Box::new(e))}



    rule binary_op() -> Node = precedence!{
        a:@ _ "==" _ b:(@) { Node::Eq(Box::new(a), Box::new(b)) }
        // a:@ _ "!=" _ b:(@) { Node::Ne(Box::new(a), Box::new(b)) }
        a:@ _ "<"  _ b:(@) { Node::Lt(Box::new(a), Box::new(b)) }
        a:@ _ "<=" _ b:(@) { Node::Le(Box::new(a), Box::new(b)) }
        // a:@ _ ">"  _ b:(@) { Node::Gt(Box::new(a), Box::new(b)) }
        // a:@ _ ">=" _ b:(@) { Node::Ge(Box::new(a), Box::new(b)) }
        --
        a:@ _ "+" _ b:(@) { Node::Add(Box::new(a), Box::new(b)) }
        a:@ _ "-" _ b:(@) { Node::Sub(Box::new(a), Box::new(b)) }
        --
        a:@ _ "*" _ b:(@) { Node::Mul(Box::new(a), Box::new(b)) }
        a:@ _ "/" _ b:(@) { Node::Div(Box::new(a), Box::new(b)) }
        a:@ _ "%" _ b:(@) { Node::Rem(Box::new(a), Box::new(b)) }
        --
        // i:index() { Node::Index(i.0, i.1) }
        // i:dot() { Node::Dot(i.0, i.1) }
        i:identifier() _ "(" args:((_ e:expression() _ {e}) ** ",") ")" { Node::Call(i, args) }
        p:primary() { Node::Load(Box::new(p)) }
        l:literal() { l }
    }

    rule primary() -> Node = precedence! {
        a:@ _ "[" _ b:expression() "]" { Node::Index2(Box::new(a), Box::new(b)) }
        a:(@) _ "." _ b:@ { Node::Dot2(Box::new(a), Box::new(b)) }
        --
        i:identifier() { Node::Identifier(i) }
    }

    rule index() -> (String, Vec<Node>)
        = i:identifier() _ "[" indices:((_ e:expression() _ {e}) ** ",") "]" { (i, indices) }

    rule dot() -> (String, Vec<String>)
        = i:identifier() _ "." fields:((_ e:identifier() _ {e}) ** ".") { (i, fields) }

    rule return_stmt() -> Node
        = "return" _ e:expression() _ ";" { Node::Return(Box::new(e)) }

    rule identifier() -> String
        = quiet!{ n:$(['a'..='z' | 'A'..='Z' | '_']['a'..='z' | 'A'..='Z' | '0'..='9' | '_']*) { n.to_owned() } }
        / expected!("identifier")

    rule literal() -> Node
        = n:$(['0'..='9']+) { Node::Number(n.parse().unwrap()) }
        // / "&" i:identifier() { Node::GlobalDataAddr(i) }

    rule number_usize() -> usize
        = n:$(['0'..='9']+) { n.parse().unwrap() }

    rule types() -> Type
        = "i32" { Type::Int32 }
        / "i64" { Type::Int64 }
        / "void" { Type::Void }
        / "struct" _ name:identifier() { Type::Struct(name) }
        / "[" _ s:number_usize() _ "]" _ t:types() { Type::Array(s, Box::new(t)) }

    rule _() =  quiet!{[' ' | '\t' | '\n']*}
});
