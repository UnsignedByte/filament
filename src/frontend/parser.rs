#![allow(clippy::upper_case_acronyms)]

//! Parser for Calyx programs.
use crate::errors::{self, FilamentResult};

use crate::core::{self, Id};
use pest_consume::{match_nodes, Error, Parser};
use std::fs;
use std::path::Path;
use std::rc::Rc;

/// Data associated with parsing the file.
#[derive(Clone)]
struct UserData {
    /// Input to the parser
    pub input: Rc<str>,
    /// Path of the file
    pub file: Rc<str>,
}

type ParseResult<T> = Result<T, Error<Rule>>;
// user data is the input program so that we can create ir::Id's
// that have a reference to the input string
type Node<'i> = pest_consume::Node<'i, Rule, UserData>;

// include the grammar file so that Cargo knows to rebuild this file on grammar changes
const _GRAMMAR: &str = include_str!("syntax.pest");

pub enum ExtOrComp {
    Ext(core::Signature),
    Comp(core::Component),
}

#[derive(Parser)]
#[grammar = "frontend/syntax.pest"]
pub struct FilamentParser;

impl FilamentParser {
    /// Parse a Calyx program into an AST representation.
    pub fn parse_file(path: &Path) -> FilamentResult<core::Namespace> {
        let content = &fs::read(path).map_err(|err| {
            errors::Error::invalid_file(format!(
                "Failed to read {}: {}",
                path.to_string_lossy(),
                err
            ))
        })?;
        let string_content = std::str::from_utf8(content)?;

        let user_data = UserData {
            input: Rc::from(string_content),
            file: Rc::from(path.to_string_lossy()),
        };
        let inputs = FilamentParser::parse_with_userdata(
            Rule::file,
            string_content,
            user_data,
        )
        .map_err(|e| e.with_path(&path.to_string_lossy()))?;
        let input = inputs.single()?;
        Ok(FilamentParser::file(input)?)
    }

    fn get_span(node: &Node) -> errors::Span {
        let ud = node.user_data();
        errors::Span::new(
            node.as_span(),
            Rc::clone(&ud.file),
            Rc::clone(&ud.input),
        )
    }
}

#[pest_consume::parser]
impl FilamentParser {
    fn EOI(_input: Node) -> ParseResult<()> {
        Ok(())
    }

    // ================ Literals =====================
    fn identifier(input: Node) -> ParseResult<Id> {
        Ok(Id::from(input.as_str()))
    }

    fn char(input: Node) -> ParseResult<&str> {
        Ok(input.as_str())
    }

    fn string_lit(input: Node) -> ParseResult<String> {
        Ok(match_nodes!(
            input.into_children();
            [char(c)..] => c.collect::<Vec<_>>().join("")
        ))
    }

    fn bitwidth(input: Node) -> ParseResult<u64> {
        input
            .as_str()
            .parse::<u64>()
            .map_err(|_| input.error("Expected valid bitwidth"))
    }

    // ================ Intervals =====================
    fn time_base(input: Node) -> ParseResult<core::IntervalTime> {
        Ok(match_nodes!(
            input.into_children();
            [identifier(var)] => core::IntervalTime::abs(var),
            [bitwidth(time)] => core::IntervalTime::concrete(time),
            [identifier(l), bitwidth(r)] => core::IntervalTime::binop_add(l, r),
        ))
    }

    fn time(input: Node) -> ParseResult<core::IntervalTime> {
        Ok(match_nodes!(
            input.into_children();
            [time(l), time(r)] => core::IntervalTime::binop_max(l, r),
            [time_base(time)] => time
        ))
    }

    fn interval(input: Node) -> ParseResult<core::Interval> {
        Ok(match_nodes!(
            input.into_children();
            [time(start), time(end)] => {
                core::Interval {
                    start,
                    end,
                }
            }
        ))
    }

    // ================ Signature =====================

    fn port_def(input: Node) -> ParseResult<core::PortDef> {
        Ok(match_nodes!(
            input.into_children();
            [interval(liveness), identifier(name), bitwidth(bitwidth)] => {
                core::PortDef {
                    liveness, name, bitwidth
                }
            }
        ))
    }

    fn abstract_var(input: Node) -> ParseResult<Vec<Id>> {
        Ok(match_nodes!(
            input.into_children();
            [identifier(vars)..] => vars.collect(),
        ))
    }

    fn ports(input: Node) -> ParseResult<Vec<core::PortDef>> {
        Ok(match_nodes!(
            input.into_children();
            [port_def(ins)..] => ins.collect()
        ))
    }

    fn arrow(input: Node) -> ParseResult<()> {
        Ok(())
    }

    fn io(
        input: Node,
    ) -> ParseResult<(Vec<core::PortDef>, Vec<core::PortDef>)> {
        Ok(match_nodes!(
            input.into_children();
            [arrow(_)] => (vec![], vec![]),
            [ports(ins), arrow(_)] =>  (ins, vec![]),
            [arrow(_), ports(outs)] =>  (vec![], outs),
            [ports(ins), arrow(_), ports(outs)] => (ins, outs),
        ))
    }

    // ================ Cells =====================
    fn instance(input: Node) -> ParseResult<core::Instance> {
        Ok(match_nodes!(
            input.into_children();
            [identifier(name), identifier(component)] => core::Instance {
                name, component
            }
        ))
    }

    // ================ Assignments =====================
    fn port(input: Node) -> ParseResult<core::Port> {
        Ok(match_nodes!(
            input.into_children();
            [bitwidth(constant)] => core::Port::Constant(constant),
            [identifier(name)] => core::Port::ThisPort(name),
            [identifier(comp), identifier(name)] => core::Port::CompPort {
                comp, name
            }
        ))
    }

    fn arguments(input: Node) -> ParseResult<Vec<core::Port>> {
        Ok(match_nodes!(
            input.into_children();
            [] => vec![],
            [port(ports)..] => ports.collect(),
        ))
    }

    fn time_args(input: Node) -> ParseResult<Vec<core::IntervalTime>> {
        Ok(match_nodes!(
            input.into_children();
            [time(args)..] => args.collect(),
        ))
    }

    fn invocation_expr(input: Node) -> ParseResult<core::Invocation> {
        let span = Self::get_span(&input);
        Ok(match_nodes!(
            input.into_children();
            [
                identifier(comp),
                time_args(abstract_vars),
                arguments(ports)
            ] => {
                core::Invocation::new(comp, abstract_vars, ports).with_span(span)
            }
        ))
    }

    fn invocation(input: Node) -> ParseResult<core::Invoke> {
        Ok(match_nodes!(
            input.into_children();
            [
                identifier(bind),
                invocation_expr(rhs)
            ] => core::Invoke {
                bind, rhs
            }
        ))
    }

    fn gt(input: Node) -> ParseResult<()> {
        Ok(())
    }
    fn lt(input: Node) -> ParseResult<()> {
        Ok(())
    }
    fn eq(input: Node) -> ParseResult<()> {
        Ok(())
    }
    fn order_op(input: Node) -> ParseResult<core::OrderOp> {
        Ok(match_nodes!(
            input.into_children();
            [gt(_)] => core::OrderOp::Gt,
            [lt(_)] => core::OrderOp::Lt,
            [eq(_)] => core::OrderOp::Eq,
        ))
    }
    fn constraint(input: Node) -> ParseResult<core::Constraint> {
        Ok(match_nodes!(
            input.into_children();
            [
                time(left),
                order_op(op),
                time(right)
            ] => core::Constraint {
                left, right, op
            }
        ))
    }
    fn constraints(input: Node) -> ParseResult<Vec<core::Constraint>> {
        Ok(match_nodes!(
            input.into_children();
            [] => Vec::default(),
            [constraint(cons)..] => cons.collect()
        ))
    }

    // ================ Component =====================
    fn signature(input: Node) -> ParseResult<core::Signature> {
        Ok(match_nodes!(
            input.into_children();
            [
                identifier(name),
                abstract_var(abstract_vars),
                io(io),
                constraints(constraints)
            ] => {
                let (inputs, outputs) = io;
                core::Signature {
                    name,
                    abstract_vars,
                    inputs,
                    outputs,
                    constraints,
                }
            },
            [
                identifier(name),
                io(io),
                constraints(constraints)
            ] => {
                let (inputs, outputs) = io;
                core::Signature {
                    name,
                    abstract_vars: vec![],
                    inputs,
                    outputs,
                    constraints
                }
            }
        ))
    }

    fn when(input: Node) -> ParseResult<core::When> {
        Ok(match_nodes!(
            input.into_children();
            [time(time), command(body)..] => core::When {
                time, commands: body.collect()
            }
        ))
    }

    fn connect(input: Node) -> ParseResult<core::Connect> {
        let span = Self::get_span(&input);
        Ok(match_nodes!(
            input.into_children();
            [port(dst), port(src)] => core::Connect::new(dst, src).with_span(span)
        ))
    }

    fn command(input: Node) -> ParseResult<core::Command> {
        Ok(match_nodes!(
            input.into_children();
            [invocation(assign)] => core::Command::Invoke(assign),
            [instance(cell)] => core::Command::Instance(cell),
            [when(wh)] => core::Command::When(wh),
            [connect(con)] => core::Command::Connect(con),
        ))
    }

    fn component(input: Node) -> ParseResult<core::Component> {
        Ok(match_nodes!(
            input.into_children();
            [
                signature(sig),
                command(body)..
            ] => {
                core::Component {
                    sig: Rc::new(sig),
                    body: body.collect(),
                }
            }
        ))
    }

    fn external(input: Node) -> ParseResult<core::Signature> {
        Ok(match_nodes!(
            input.into_children();
            [signature(sig)] => sig,
        ))
    }

    fn comp_or_ext(input: Node) -> ParseResult<ExtOrComp> {
        Ok(match_nodes!(
            input.into_children();
            [external(sig)] => ExtOrComp::Ext(sig),
            [component(comp)] => ExtOrComp::Comp(comp),
        ))
    }

    fn imports(input: Node) -> ParseResult<Vec<String>> {
        Ok(match_nodes!(
            input.into_children();
            [string_lit(path)..] => path.collect()
        ))
    }

    fn file(input: Node) -> ParseResult<core::Namespace> {
        Ok(match_nodes!(
            input.into_children();
            [imports(imps), comp_or_ext(mixed).., _EOI] => {
                let mut namespace = core::Namespace {
                    imports: imps,
                    signatures: vec![],
                    components: vec![],
                };
                for m in mixed {
                    match m {
                        ExtOrComp::Ext(sig) => namespace.signatures.push(sig),
                        ExtOrComp::Comp(comp) => namespace.components.push(comp),
                    }
                }
                namespace
            }
        ))
    }
}
