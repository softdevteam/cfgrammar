// Copyright (c) 2017 King's College London
// created by the Software Development Team <http://soft-dev.org/>
//
// The Universal Permissive License (UPL), Version 1.0
//
// Subject to the condition set forth below, permission is hereby granted to any person obtaining a
// copy of this software, associated documentation and/or data (collectively the "Software"), free
// of charge and under any and all copyright rights in the Software, and any and all patent rights
// owned or freely licensable by each licensor hereunder covering either (i) the unmodified
// Software as contributed to or provided by such licensor, or (ii) the Larger Works (as defined
// below), to deal in both
//
// (a) the Software, and
// (b) any piece of software and/or hardware listed in the lrgrwrks.txt file
// if one is included with the Software (each a "Larger Work" to which the Software is contributed
// by such licensors),
//
// without restriction, including without limitation the rights to copy, create derivative works
// of, display, perform, and distribute the Software and make, use, sell, offer for sale, import,
// export, have made, and have sold the Software and the Larger Work(s), and to sublicense the
// foregoing rights on either these or other terms.
//
// This license is subject to the following condition: The above copyright notice and either this
// complete permission notice or at a minimum a reference to the UPL must be included in all copies
// or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
// BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
// NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;

use {Grammar, NTIdx, PIdx, Symbol, TIdx};
use super::YaccKind;

const START_NONTERM         : &str = "^";
const IMPLICIT_NONTERM      : &str = "~";
const IMPLICIT_START_NONTERM: &str = "^~";

use yacc::ast;
use yacc::ast::GrammarValidationError;
use yacc::parser::YaccParserError;

pub type PrecedenceLevel = u64;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Precedence {
    pub level: PrecedenceLevel,
    pub kind:  AssocKind
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AssocKind {
    Left,
    Right,
    Nonassoc
}

/// Representation of a `YaccGrammar`. See the [top-level documentation](../../index.html) for the
/// guarantees this struct makes about nonterminals, terminals, productions, and symbols.
pub struct YaccGrammar {
    /// How many nonterminals does this grammar have?
    nonterms_len: u32,
    /// A mapping from `NTIdx` -> `String`.
    nonterm_names: Vec<String>,
    /// A mapping from `TIdx` -> `Option<String>`. Every user-specified terminal will have a name,
    /// but terminals inserted by cfgrammar (e.g. the EOF terminal) won't.
    term_names: Vec<Option<String>>,
    /// A mapping from `TIdx` -> `Option<Precedence>`
    term_precs: Vec<Option<Precedence>>,
    /// How many terminals does this grammar have?
    terms_len: u32,
    /// The offset of the EOF terminal.
    eof_term_idx: TIdx,
    /// How many productions does this grammar have?
    prods_len: u32,
    /// Which production is the sole production of the start rule?
    start_prod: PIdx,
    /// A list of all productions.
    prods: Vec<Vec<Symbol>>,
    /// A mapping from rules to their productions. Note that 1) the order of rules is identical to
    /// that of `nonterm_names` 2) every rule will have at least 1 production 3) productions
    /// are not necessarily stored sequentially.
    rules_prods: Vec<Vec<PIdx>>,
    /// A mapping from productions to their corresponding rule indexes.
    prods_rules: Vec<NTIdx>,
    /// The precedence of each production.
    prod_precs: Vec<Option<Precedence>>,
    /// The index of the nonterminal added for implicit tokens, if they were specified; otherwise
    /// `None`.
    implicit_nonterm: Option<NTIdx>
}

// Internally, we assume that a grammar's start rule has a single production. Since we manually
// create the start rule ourselves (without relying on user input), this is a safe assumption.

impl YaccGrammar {
    /// Translate a `GrammarAST` into a `YaccGrammar`. This function is akin to the part a traditional
    /// compiler that takes in an AST and converts it into a binary.
    ///
    /// As we're compiling the `GrammarAST` into a `Grammar` we add a new start rule (which we'll
    /// refer to as `^`, though the actual name is a fresh name that is guaranteed to be unique)
    /// that references the user defined start rule.
    pub fn new(yacc_kind: YaccKind, ast: &ast::GrammarAST) -> YaccGrammar {
        let mut nonterm_names: Vec<String> = Vec::with_capacity(ast.rules.len() + 1);

        // Generate a guaranteed unique start nonterm name. We simply keep making the string longer
        // until we've hit something unique (at the very worst, this will require looping for as
        // many times as there are nonterminals). We use the same technique later for unique end
        // term and whitespace names.
        let mut start_nonterm = START_NONTERM.to_string();
        while ast.rules.get(&start_nonterm).is_some() {
            start_nonterm += START_NONTERM;
        }
        nonterm_names.push(start_nonterm.clone());

        let implicit_nonterm;
        let implicit_start_nonterm;
        match yacc_kind {
            YaccKind::Original => {
                implicit_nonterm = None;
                implicit_start_nonterm = None;
            },
            YaccKind::Eco => {
                if ast.implicit_tokens.is_some() {
                    let mut n1 = IMPLICIT_NONTERM.to_string();
                    while ast.rules.get(&n1).is_some() {
                        n1 += IMPLICIT_NONTERM;
                    }
                    nonterm_names.push(n1.clone());
                    implicit_nonterm = Some(n1);
                    let mut n2 = IMPLICIT_START_NONTERM.to_string();
                    while ast.rules.get(&n2).is_some() {
                        n2 += IMPLICIT_START_NONTERM;
                    }
                    nonterm_names.push(n2.clone());
                    implicit_start_nonterm = Some(n2);
                }
                else {
                    implicit_nonterm = None;
                    implicit_start_nonterm = None;
                }
            }
        };

        for k in ast.rules.keys() {
            nonterm_names.push(k.clone());
        }
        let mut rules_prods:Vec<Vec<PIdx>> = Vec::with_capacity(nonterm_names.len());
        let mut nonterm_map = HashMap::<String, NTIdx>::new();
        for (i, v) in nonterm_names.iter().enumerate() {
            rules_prods.push(Vec::new());
            nonterm_map.insert(v.clone(), NTIdx::from(i));
        }

        let mut term_names: Vec<Option<String>> = Vec::with_capacity(ast.tokens.len() + 1);
        let mut term_precs: Vec<Option<Precedence>> = Vec::with_capacity(ast.tokens.len() + 1);
        for k in &ast.tokens {
            term_names.push(Some(k.clone()));
            term_precs.push(ast.precs.get(k).cloned());
        }
        let eof_term_idx = TIdx::from(term_names.len());
        term_names.push(None);
        term_precs.push(None);
        let mut term_map = HashMap::<String, TIdx>::new();
        for (i, v) in term_names.iter().enumerate() {
            if let Some(n) = v.as_ref() {
               term_map.insert(n.clone(), TIdx::from(i));
            }
        }

        // In order to avoid fiddling about with production indices from the AST, we simply map
        // them 1:1 to grammar indices. That means that any new productions are added to the *end*
        // of the list of productions.
        let mut prods = vec![None; ast.prods.len()];
        let mut prod_precs: Vec<Option<Option<Precedence>>> = vec![None; ast.prods.len()];
        let mut prods_rules = vec![None; ast.prods.len()];
        for astrulename in &nonterm_names {
            let rule_idx = nonterm_map[astrulename];
            if astrulename == &start_nonterm {
                // Add the special start rule which has a single production which references a
                // single nonterminal.
                rules_prods.get_mut(usize::from(nonterm_map[astrulename]))
                           .unwrap()
                           .push(prods.len().into());
                let start_prod = match implicit_start_nonterm {
                    None => {
                        // Add ^: S;
                        vec![Symbol::Nonterm(nonterm_map[ast.start.as_ref().unwrap()])]
                    }
                    Some(ref s) => {
                        // An implicit rule has been specified, so the special start rule
                        // needs to reference the intermediate start rule required. Therefore add:
                        //   ^: ^~;
                        vec![Symbol::Nonterm(nonterm_map[s])]
                    }
                };
                prods.push(Some(start_prod));
                prod_precs.push(Some(None));
                prods_rules.push(Some(rule_idx));
                continue;
            }
            else if implicit_start_nonterm.as_ref().map_or(false, |s| s == astrulename) {
                // Add the intermediate start rule (handling implicit tokens at the beginning of
                // the file):
                //   ^~: ~ S;
                rules_prods.get_mut(usize::from(nonterm_map[astrulename]))
                           .unwrap()
                           .push(prods.len().into());
                prods.push(Some(vec![Symbol::Nonterm(nonterm_map[implicit_nonterm.as_ref().unwrap()]),
                                     Symbol::Nonterm(nonterm_map[ast.start.as_ref().unwrap()])]));
                prod_precs.push(Some(None));
                prods_rules.push(Some(rule_idx));
                continue;
            }
            else if implicit_nonterm.as_ref().map_or(false, |s| s == astrulename) {
                // Add the implicit rule: ~: "IMPLICIT_TERM1" ~ | ... | "IMPLICIT_TERMN" ~ | ;
                let implicit_prods = &mut rules_prods[usize::from(nonterm_map[astrulename])];
                // Add a production for each implicit terminal
                for t in ast.implicit_tokens.as_ref().unwrap().iter() {
                    implicit_prods.push(prods.len().into());
                    prods.push(Some(vec![Symbol::Term(term_map[t]), Symbol::Nonterm(rule_idx)]));
                    prod_precs.push(Some(None));
                    prods_rules.push(Some(rule_idx));
                }
                // Add an empty production
                implicit_prods.push(prods.len().into());
                prods.push(Some(vec![]));
                prod_precs.push(Some(None));
                prods_rules.push(Some(rule_idx));
                continue;
            }

            let rule = &mut rules_prods[usize::from(rule_idx)];
            for &prod_idx in &ast.rules[astrulename] {
                let astprod = &ast.prods[prod_idx];
                let mut prod = Vec::with_capacity(astprod.symbols.len());
                for astsym in &astprod.symbols {
                    match *astsym {
                        ast::Symbol::Nonterm(ref n) => {
                            prod.push(Symbol::Nonterm(nonterm_map[n]));
                        },
                        ast::Symbol::Term(ref n) => {
                            prod.push(Symbol::Term(term_map[n]));
                            if implicit_nonterm.is_some() {
                                prod.push(Symbol::Nonterm(nonterm_map[&implicit_nonterm.clone().unwrap()]));
                            }
                        }
                    };
                }
                let mut prec = None;
                if let Some(ref n) = astprod.precedence {
                    prec = Some(ast.precs[n]);
                } else {
                    for astsym in astprod.symbols.iter().rev() {
                        if let ast::Symbol::Term(ref n) = *astsym {
                            if let Some(p) = ast.precs.get(n) {
                                prec = Some(*p);
                            }
                            break;
                        }
                    }
                }
                (*rule).push(prod_idx.into());
                prods[prod_idx] = Some(prod);
                prod_precs[prod_idx] = Some(prec);
                prods_rules[prod_idx] = Some(rule_idx);
            }
        }

        YaccGrammar{
            nonterms_len:     u32::try_from(nonterm_names.len()).unwrap(),
            nonterm_names,
            terms_len:        u32::try_from(term_names.len()).unwrap(),
            eof_term_idx,
            term_names,
            term_precs,
            prods_len:        u32::try_from(prods.len()).unwrap(),
            start_prod:       rules_prods[usize::from(nonterm_map[&start_nonterm])][0],
            rules_prods,
            prods_rules:      prods_rules.into_iter().map(|x| x.unwrap()).collect(),
            prods:            prods.into_iter().map(|x| x.unwrap()).collect(),
            prod_precs:       prod_precs.into_iter().map(|x| x.unwrap()).collect(),
            implicit_nonterm: implicit_nonterm.and_then(|x| Some(nonterm_map[&x]))
        }
    }

    /// Return the index of the end terminal.
    pub fn eof_term_idx(&self) -> TIdx {
        self.eof_term_idx
    }

    /// Return the productions for nonterminal `i`. Panics if `i` doesn't exist.
    pub fn nonterm_to_prods(&self, i: NTIdx) -> &[PIdx] {
        &self.rules_prods[usize::from(i)]
    }

    /// Return the name of nonterminal `i`. Panics if `i` doesn't exist.
    pub fn nonterm_name(&self, i: NTIdx) -> &str {
        &self.nonterm_names[usize::from(i)]
    }

    /// Return an iterator which produces (in no particular order) all this grammar's valid `NTIdx`s.
    pub fn iter_nonterm_idxs(&self) -> Box<Iterator<Item=NTIdx>> {
        Box::new((0..self.nonterms_len).map(NTIdx::from))
    }

    /// Get the sequence of symbols for production `i`. Panics if `i` doesn't exist.
    pub fn prod(&self, i: PIdx) -> &[Symbol] {
        &self.prods[usize::from(i)]
    }

    /// Return the nonterm index of the production `i`. Panics if `i` doesn't exist.
    pub fn prod_to_nonterm(&self, i: PIdx) -> NTIdx {
        self.prods_rules[usize::from(i)]
    }

    /// Return the precedence of production `i` (where `None` indicates "no precedence specified").
    /// Panics if `i` doesn't exist.
    pub fn prod_precedence(&self, i: PIdx) -> Option<Precedence> {
        self.prod_precs[usize::from(i)]
    }

    /// Return the name of terminal `i` (where `None` indicates "the rule has no name"). Panics if
    /// `i` doesn't exist.
    pub fn term_name(&self, i: TIdx) -> Option<&str> {
        self.term_names[usize::from(i)].as_ref().and_then(|x| Some(&x[..]))
    }

    /// Return the precedence of terminal `i` (where `None` indicates "no precedence specified").
    /// Panics if `i` doesn't exist.
    pub fn term_precedence(&self, i: TIdx) -> Option<Precedence> {
        self.term_precs[usize::from(i)]
    }

    /// Returns a map from names to `TIdx`s of all tokens that a lexer will need to generate valid
    /// inputs from this grammar.
    pub fn terms_map(&self) -> HashMap<&str, TIdx> {
        let mut m = HashMap::with_capacity(self.terms_len as usize - 1);
        for i in 0..self.terms_len as usize {
            if let Some(n) = self.term_names[i].as_ref() {
                m.insert(&**n, TIdx::from(i));
            }
        }
        m
    }

    /// Return the production index of the start rule's sole production (for Yacc grammars the
    /// start rule is defined to have precisely one production).
    pub fn start_prod(&self) -> PIdx {
        self.start_prod
    }

    /// Return the `NTIdx` of the implict nonterm if it exists, or `None` otherwise.
    pub fn implicit_nonterm(&self) -> Option<NTIdx> {
        self.implicit_nonterm
    }

    /// Return the index of the nonterminal named `n` or `None` if it doesn't exist.
    pub fn nonterm_idx(&self, n: &str) -> Option<NTIdx> {
        self.nonterm_names.iter()
                          .position(|x| x == n)
                          .map(|x| NTIdx::from(x))
    }

    /// Return the index of the terminal named `n` or `None` if it doesn't exist.
    pub fn term_idx(&self, n: &str) -> Option<TIdx> {
        self.term_names.iter()
                       .position(|x| x.as_ref().map_or(false, |x| x == n))
                       .map(|x| TIdx::from(x))
    }

    /// Is there a path from the `from` non-term to the `to` non-term? Note that recursive rules
    /// return `true` for a path from themselves to themselves.
    pub fn has_path(&self, from: NTIdx, to: NTIdx) -> bool {
        let mut seen = vec![];
        seen.resize(self.nonterms_len() as usize, false);
        let mut todo = vec![];
        todo.resize(self.nonterms_len() as usize, false);
        todo[usize::from(from)] = true;
        loop {
            let mut empty = true;
            for i in 0..self.nonterms_len() as usize {
                if !todo[i] {
                    continue;
                }
                seen[i] = true;
                todo[i] = false;
                empty = false;
                for p_idx in self.nonterm_to_prods(NTIdx::from(i)).iter() {
                    for sym in self.prod(*p_idx) {
                        if let Symbol::Nonterm(nt_idx) = *sym {
                            if nt_idx == to {
                                return true;
                            }
                            if !seen[usize::from(nt_idx)] {
                                todo[usize::from(nt_idx)] = true;
                            }
                        }
                    }
                }
            }
            if empty {
                return false;
            }
        }
    }

    /// Return a `SentenceGenerator` which can then generate minimal sentences for any non-term
    /// based on the user-defined `term_cost` function which gives the associated cost for
    /// generating each terminal (where the cost must be greater than 0). Note that multiple
    /// terminals can have the same score. The simplest cost function is thus `|_| 1`.
    pub fn sentence_generator<F>(&self, term_cost: F) -> SentenceGenerator
                        where F: Fn(TIdx) -> u8
    {
        SentenceGenerator::new(self, term_cost)
    }

}

impl Grammar for YaccGrammar {
    fn prods_len(&self) -> u32 {
        self.prods_len
    }

    fn terms_len(&self) -> u32 {
        self.terms_len
    }

    fn nonterms_len(&self) -> u32 {
        self.nonterms_len
    }

    /// Return the index of the start rule.
    fn start_rule_idx(&self) -> NTIdx {
        self.prod_to_nonterm(self.start_prod)
    }
}

/// A `SentenceGenerator` can generate minimal sentences for any given non-terminal. e.g. for the
/// grammar:
///
/// ```text
/// %start A
/// %%
/// A: A B | ;
/// B: C | D;
/// C: 'x' B | 'x';
/// D: 'y' B | 'y' 'z';
/// ```
///
/// the following are valid minimal sentences:
///
/// ```text
/// A: []
/// B: [x]
/// C: [x]
/// D: [y, x] or [y, z]
/// ```
pub struct SentenceGenerator<'a> {
    grm: &'a YaccGrammar,
    nonterm_min_costs: RefCell<Option<Vec<u32>>>,
    nonterm_max_costs: RefCell<Option<Vec<u32>>>,
    term_costs: Vec<u8>
}

impl<'a> SentenceGenerator<'a> {
    fn new<F>(grm: &YaccGrammar, term_cost: F) -> SentenceGenerator
        where F: Fn(TIdx) -> u8
    {
        let mut term_costs = Vec::with_capacity(grm.terms_len() as usize);
        for i in 0..grm.terms_len() {
            term_costs.push(term_cost(TIdx::from(i)));
        }
        SentenceGenerator{grm,
                          term_costs,
                          nonterm_min_costs: RefCell::new(None),
                          nonterm_max_costs: RefCell::new(None)}
    }

    /// What is the cost of a minimal sentence for the non-terminal `nonterm_idx`? Note that,
    /// unlike `min_sentence`, this function does not actually *build* a sentence and it is thus
    /// much faster.
    pub fn min_sentence_cost(&self, nonterm_idx: NTIdx) -> u32 {
        self.nonterm_min_costs.borrow_mut()
                              .get_or_insert_with(|| nonterm_min_costs(self.grm,
                                                                       &self.term_costs))
                              [usize::from(nonterm_idx)]
    }

    /// What is the cost of a maximal sentence for the non-terminal `nonterm_idx`? Non-terminals
    /// which can generate sentences of unbounded length return None; non-terminals which can only
    /// generate maximal strings of a finite length return a `Some(u32)`.
    pub fn max_sentence_cost(&self, nonterm_idx: NTIdx) -> Option<u32> {
        let v = self.nonterm_max_costs.borrow_mut()
                                      .get_or_insert_with(||
                                           nonterm_max_costs(self.grm, &self.term_costs))
                                      [usize::from(nonterm_idx)];
        if v == u32::max_value() {
            None
        } else {
            Some(v)
        }
    }

    /// Non-deterministically return a minimal sentence from the set of minimal sentences for the
    /// non-terminal `nonterm_idx`.
    pub fn min_sentence(&self, nonterm_idx: NTIdx) -> Vec<TIdx> {
        let cheapest_prod = |nt_idx: NTIdx| -> PIdx {
            let mut low_sc = None;
            let mut low_idx = None;
            for &pidx in self.grm.nonterm_to_prods(nt_idx).iter() {
                let mut sc = 0;
                for sym in self.grm.prod(pidx).iter() {
                    sc += match *sym {
                        Symbol::Nonterm(i) => self.min_sentence_cost(i),
                        Symbol::Term(i)    => self.term_costs[usize::from(i)] as u32
                    };
                }
                if low_sc.is_none() || sc < low_sc.unwrap() {
                    low_sc = Some(sc);
                    low_idx = Some(pidx);
                }
            }
            low_idx.unwrap()
        };

        let mut s = vec![];
        let mut st = vec![(cheapest_prod(nonterm_idx), 0)];
        while !st.is_empty() {
            let (p_idx, sym_idx) = st.pop().unwrap();
            for (i, sym) in self.grm.prod(p_idx).iter().enumerate().skip(sym_idx) {
                match *sym {
                    Symbol::Nonterm(j) => {
                        st.push((p_idx, i + 1));
                        st.push((cheapest_prod(j), 0));
                    },
                    Symbol::Term(j) => {
                        s.push(j);
                    }
                }
            }
        }
        s
    }

    /// Return (in arbitrary order) all the minimal sentences for the non-terminal `nonterm_idx`.
    pub fn min_sentences(&self, nonterm_idx: NTIdx) -> Vec<Vec<TIdx>> {
        let cheapest_prods = |nt_idx: NTIdx| -> Vec<PIdx> {
            let mut low_sc = None;
            let mut low_idxs = vec![];
            for &pidx in self.grm.nonterm_to_prods(nt_idx).iter() {
                let mut sc = 0;
                for sym in self.grm.prod(pidx).iter() {
                    sc += match *sym {
                        Symbol::Nonterm(i) => self.min_sentence_cost(i),
                        Symbol::Term(i)    => self.term_costs[usize::from(i)] as u32
                    };
                }
                if low_sc.is_none() || sc <= low_sc.unwrap() {
                    if low_sc.is_some() && sc < low_sc.unwrap() {
                        low_idxs.clear();
                    }
                    low_sc = Some(sc);
                    low_idxs.push(pidx);
                }
            }
            low_idxs
        };

        let mut sts = Vec::new(); // Output sentences
        for p_idx in cheapest_prods(nonterm_idx) {
            let prod = self.grm.prod(p_idx);
            if prod.is_empty() {
                sts.push(vec![]);
                continue;
            }

            // We construct the minimal sentences in two phases.
            //
            // First, for each symbol in the production, we gather all the possible minimal
            // sentences for it. If, for the grammar:
            //   X: 'a' Y
            //   Y: 'b' | 'c'
            // we ask for the minimal sentences of X's only production we'll end up with a vec of
            // vecs as follows:
            //   [[['a']], [['b'], ['c']]]

            let mut ms = Vec::with_capacity(prod.len());
            for sym in prod {
                match *sym {
                    Symbol::Nonterm(nt_idx) => ms.push(self.min_sentences(nt_idx)),
                    Symbol::Term(t_idx) => ms.push(vec![vec![t_idx]])
                }
            }

            // Second, we need to generate all combinations of the gathered sentences. We do this
            // by writing our own simple numeric incrementing scheme. If we rewrite the list from
            // above as follows:
            //
            //      0 1 <- call this axis "i"
            //   0: a b
            //   1:   c
            //   ^
            //   |
            //   call this axis "todo"
            //
            // this hopefully becomes easier to see. Let's call the list "ms": the combinations we
            // need to generate are thus:
            //
            //   ms[0][0] + ms[1][0]  (i.e. 'ab')
            //   ms[0][0] + ms[1][1]  (i.e. 'ac')
            //
            // The easiest way to model this is to have a list (todo) with each entry starting at
            // 0. After each iteration around the loop (i) we add 1 to the last todo column's
            // entry: if that spills over the length of the corresponding ms entry, then we reset
            // that column to zero, and try adding 1 to the previous column (as many times as
            // needed). If the first column spills, then we're done. This is basically normal
            // arithmetic but with each digit having an arbitrary base.

            let mut todo = Vec::new();
            todo.resize(prod.len(), 0);
            let mut cur = Vec::new();
            'b: loop {
                for i in 0..todo.len() {
                    cur.extend(&ms[i][todo[i]]);
                }
                sts.push(cur.drain(..).collect::<Vec<TIdx>>());

                let mut j = todo.len() - 1;
                loop {
                    if todo[j] + 1 == ms[j].len() {
                        if j == 0 {
                            break 'b;
                        }
                        todo[j] = 0;
                        j -= 1;
                    } else {
                        todo[j] += 1;
                        break;
                    }
                }
            }
        }
        sts
    }
}

/// Return the cost of a minimal string for each non-terminal in this grammar. The cost of a
/// terminal is specified by the user-defined `term_cost` function.
fn nonterm_min_costs(grm: &YaccGrammar, term_costs: &[u8]) -> Vec<u32>
{
    // We use a simple(ish) fixed-point algorithm to determine costs. We maintain two lists
    // "costs" and "done". An integer costs[i] starts at 0 and monotonically increments
    // until done[i] is true, at which point costs[i] value is fixed. We also use the done
    // list as a simple "todo" list: whilst there is at least one false value in done, there is
    // still work to do.
    //
    // On each iteration of the loop, we examine each non-terminal in the todo list to see if
    // we can get a better idea of its true cost. Some are trivial:
    //   * A non-terminal with an empty production immediately has a cost of 0.
    //   * Non-terminals whose productions don't reference any non-terminals (i.e. only contain
    //     terminals) can be immediately given a cost by calculating the lowest-cost production.
    // However if a non-terminal A references another non-terminal B, we may need to wait until
    // we've fully analysed B before we can cost A. This might seem to cause problems with
    // recursive rules, so we introduce the concept of "incomplete costs" i.e. if a production
    // references a non-terminal we can work out its minimum possible cost simply by counting
    // the production's terminal costs. Since non-terminals can have a mix of complete and
    // incomplete productions, this is sometimes enough to allow us to assign a final cost to
    // a non-terminal (if the lowest complete production's cost is lower than or equal to all
    // the lowest incomplete production's cost). This allows us to make progress, since it
    // means that we can iteratively improve our knowledge of a terminal's minimum cost:
    // eventually we will reach a point where we can determine it definitively.

    let mut costs = vec![];
    costs.resize(grm.nonterms_len() as usize, 0);
    let mut done = vec![];
    done.resize(grm.nonterms_len() as usize, false);
    loop {
        let mut all_done = true;
        for i in 0..done.len() {
            if done[i] {
                continue;
            }
            all_done = false;
            let mut ls_cmplt = None; // lowest completed cost
            let mut ls_noncmplt = None; // lowest non-completed cost
            for p_idx in grm.nonterm_to_prods(NTIdx::from(i)).iter() {
                let mut c: u32 = 0; // production cost
                let mut cmplt = true;
                for sym in grm.prod(*p_idx) {
                    let sc = match *sym {
                                 Symbol::Term(term_idx) =>
                                     term_costs[usize::from(term_idx)] as u32,
                                 Symbol::Nonterm(nt_idx) => {
                                     if !done[usize::from(nt_idx)] {
                                         cmplt = false;
                                     }
                                     costs[usize::from(nt_idx)]
                                 }
                             };
                    c = c.checked_add(sc).expect(
                            "Overflow occurred when calculating non-term costs");
                }
                if cmplt && (ls_cmplt.is_none() || c < ls_cmplt.unwrap()) {
                    ls_cmplt = Some(c);
                } else if !cmplt && (ls_noncmplt.is_none() || c < ls_noncmplt.unwrap()) {
                    ls_noncmplt = Some(c);
                }
            }
            if ls_cmplt.is_some() && (ls_noncmplt.is_none() || ls_cmplt < ls_noncmplt) {
                debug_assert!(ls_cmplt.unwrap() >= costs[i]);
                costs[i] = ls_cmplt.unwrap();
                done[i] = true;
            } else if ls_noncmplt.is_some() {
                debug_assert!(ls_noncmplt.unwrap() >= costs[i]);
                costs[i] = ls_noncmplt.unwrap();
            }
        }
        if all_done {
            debug_assert!(done.iter().all(|x| *x));
            break;
        }
    }
    costs
}


/// Return the cost of the maximal string for each non-terminal in this grammar (`u32::max_value()`
/// representing "this non-terminal can generate strings of infinite length"). The cost of a
/// terminal is specified by the user-defined `term_cost` function.
fn nonterm_max_costs(grm: &YaccGrammar, term_costs: &[u8]) -> Vec<u32>
{
    let mut done = vec![];
    done.resize(grm.nonterms_len() as usize, false);
    let mut costs = vec![];
    costs.resize(grm.nonterms_len() as usize, 0);

    // First mark all recursive non-terminals.
    for i in 0..grm.nonterms_len() as usize {
        // Calling has_path so frequently is not exactly efficient...
        if grm.has_path(NTIdx::from(i), NTIdx::from(i)) {
            costs[i] = u32::max_value();
            done[i] = true;
        }
    }

    loop {
        let mut all_done = true;
        for i in 0..done.len() {
            if done[i] {
                continue;
            }
            all_done = false;
            let mut hs_cmplt = None; // highest completed cost
            let mut hs_noncmplt = None; // highest non-completed cost
            'a: for p_idx in grm.nonterm_to_prods(NTIdx::from(i)).iter() {
                let mut c: u32 = 0; // production cost
                let mut cmplt = true;
                for sym in grm.prod(*p_idx) {
                    let sc = match *sym {
                                 Symbol::Term(term_idx) =>
                                     term_costs[usize::from(term_idx)] as u32,
                                 Symbol::Nonterm(nt_idx) => {
                                     if costs[usize::from(nt_idx)] == u32::max_value() {
                                         // As soon as we find reference to an infinite
                                         // non-terminal, we can stop looking.
                                         hs_cmplt = Some(u32::max_value());
                                         break 'a;
                                     }
                                     if !done[usize::from(nt_idx)] {
                                         cmplt = false;
                                     }
                                     costs[usize::from(nt_idx)]
                                 }
                             };
                    c = c.checked_add(sc).expect(
                            "Overflow occurred when calculating non-term costs");
                    if c == u32::max_value() {
                        panic!("Unable to represent cost in 64 bits.");
                    }
                }
                if cmplt && (hs_cmplt.is_none() || c > hs_cmplt.unwrap()) {
                    hs_cmplt = Some(c);
                } else if !cmplt && (hs_noncmplt.is_none() || c > hs_noncmplt.unwrap()) {
                    hs_noncmplt = Some(c);
                }
            }
            if hs_cmplt.is_some() && (hs_noncmplt.is_none() || hs_cmplt > hs_noncmplt) {
                debug_assert!(hs_cmplt.unwrap() >= costs[i]);
                costs[i] = hs_cmplt.unwrap();
                done[i] = true;
            } else if hs_noncmplt.is_some() {
                debug_assert!(hs_noncmplt.unwrap() >= costs[i]);
                costs[i] = hs_noncmplt.unwrap();
            }
        }
        if all_done {
            debug_assert!(done.iter().all(|x| *x));
            break;
        }
    }
    costs
}

#[derive(Debug)]
pub enum YaccGrammarError {
    YaccParserError(YaccParserError),
    GrammarValidationError(GrammarValidationError)
}

impl From<YaccParserError> for YaccGrammarError {
    fn from(err: YaccParserError) -> YaccGrammarError {
        YaccGrammarError::YaccParserError(err)
    }
}

impl From<GrammarValidationError> for YaccGrammarError {
    fn from(err: GrammarValidationError) -> YaccGrammarError {
        YaccGrammarError::GrammarValidationError(err)
    }
}

impl fmt::Display for YaccGrammarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            YaccGrammarError::YaccParserError(ref e) => e.fmt(f),
            YaccGrammarError::GrammarValidationError(ref e) => e.fmt(f)
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use super::{IMPLICIT_NONTERM, IMPLICIT_START_NONTERM, nonterm_max_costs, nonterm_min_costs};
    use {NTIdx, PIdx, Symbol, TIdx};
    use yacc::{AssocKind, Precedence, yacc_grm, YaccKind};

    #[test]
    fn test_minimal() {
        let grm = yacc_grm(YaccKind::Original,
                           "%start R %token T %% R: 'T';").unwrap();

        assert_eq!(grm.start_prod, PIdx::from(1 as u32));
        assert_eq!(grm.implicit_nonterm(), None);
        grm.nonterm_idx("^").unwrap();
        grm.nonterm_idx("R").unwrap();
        grm.term_idx("T").unwrap();

        assert_eq!(grm.rules_prods, vec![vec![PIdx::from(1 as u32)], vec![PIdx::from(0 as u32)]]);
        let start_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("^").unwrap())][0]);
        assert_eq!(*start_prod, [Symbol::Nonterm(grm.nonterm_idx("R").unwrap())]);
        let r_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("R").unwrap())][0]);
        assert_eq!(*r_prod, [Symbol::Term(grm.term_idx("T").unwrap())]);
        assert_eq!(grm.prods_rules, vec![NTIdx::from(1 as u32), NTIdx::from(0 as u32)]);

        assert_eq!(grm.terms_map(), [("T", TIdx::from(0 as u32))].iter()
                                                                 .cloned()
                                                                 .collect::<HashMap<&str, TIdx>>());
        assert_eq!(grm.iter_nonterm_idxs().collect::<Vec<NTIdx>>(),
                   vec![NTIdx::from(0 as u32), NTIdx::from(1 as u32)]);
    }

    #[test]
    fn test_rule_ref() {
        let grm = yacc_grm(YaccKind::Original,
                           "%start R %token T %% R : S; S: 'T';").unwrap();

        grm.nonterm_idx("^").unwrap();
        grm.nonterm_idx("R").unwrap();
        grm.nonterm_idx("S").unwrap();
        grm.term_idx("T").unwrap();
        assert!(grm.term_name(grm.eof_term_idx()).is_none());

        assert_eq!(grm.rules_prods, vec![vec![PIdx::from(2 as u32)],
                                         vec![PIdx::from(0 as u32)],
                                         vec![PIdx::from(1 as u32)]]);
        let start_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("^").unwrap())][0]);
        assert_eq!(*start_prod, [Symbol::Nonterm(grm.nonterm_idx("R").unwrap())]);
        let r_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("R").unwrap())][0]);
        assert_eq!(r_prod.len(), 1);
        assert_eq!(r_prod[0], Symbol::Nonterm(grm.nonterm_idx("S").unwrap()));
        let s_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("S").unwrap())][0]);
        assert_eq!(s_prod.len(), 1);
        assert_eq!(s_prod[0], Symbol::Term(grm.term_idx("T").unwrap()));
    }

    #[test]
    fn test_long_prod() {
        let grm = yacc_grm(YaccKind::Original,
                           "%start R %token T1 T2 %% R : S 'T1' S; S: 'T2';").unwrap();

        grm.nonterm_idx("^").unwrap();
        grm.nonterm_idx("R").unwrap();
        grm.nonterm_idx("S").unwrap();
        grm.term_idx("T1").unwrap();
        grm.term_idx("T2").unwrap();

        assert_eq!(grm.rules_prods, vec![vec![PIdx::from(2 as u32)],
                                         vec![PIdx::from(0 as u32)],
                                         vec![PIdx::from(1 as u32)]]);
        assert_eq!(grm.prods_rules, vec![NTIdx::from(1 as u32),
                                         NTIdx::from(2 as u32),
                                         NTIdx::from(0 as u32)]);
        let start_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("^").unwrap())][0]);
        assert_eq!(*start_prod, [Symbol::Nonterm(grm.nonterm_idx("R").unwrap())]);
        let r_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("R").unwrap())][0]);
        assert_eq!(r_prod.len(), 3);
        assert_eq!(r_prod[0], Symbol::Nonterm(grm.nonterm_idx("S").unwrap()));
        assert_eq!(r_prod[1], Symbol::Term(grm.term_idx("T1").unwrap()));
        assert_eq!(r_prod[2], Symbol::Nonterm(grm.nonterm_idx("S").unwrap()));
        let s_prod = grm.prod(grm.rules_prods[usize::from(grm.nonterm_idx("S").unwrap())][0]);
        assert_eq!(s_prod.len(), 1);
        assert_eq!(s_prod[0], Symbol::Term(grm.term_idx("T2").unwrap()));
    }


    #[test]
    fn test_prods_rules() {
        let grm = yacc_grm(YaccKind::Original, "
            %start A
            %%
            A: B
             | C;
            B: 'x';
            C: 'y'
             | 'z';
          ").unwrap();

        assert_eq!(grm.prods_rules, vec![NTIdx::from(1 as u32),
                                         NTIdx::from(1 as u32),
                                         NTIdx::from(2 as u32),
                                         NTIdx::from(3 as u32),
                                         NTIdx::from(3 as u32),
                                         NTIdx::from(0 as u32)]);
    }

    #[test]
    fn test_left_right_nonassoc_precs() {
        let grm = yacc_grm(YaccKind::Original, "
            %start Expr
            %right '='
            %left '+' '-'
            %left '/'
            %left '*'
            %nonassoc '~'
            %%
            Expr : Expr '=' Expr
                 | Expr '+' Expr
                 | Expr '-' Expr
                 | Expr '/' Expr
                 | Expr '*' Expr
                 | Expr '~' Expr
                 | 'id' ;
          ").unwrap();

        assert_eq!(grm.prod_precs.len(), 8);
        assert_eq!(grm.prod_precs[0].unwrap(), Precedence{level: 0, kind: AssocKind::Right});
        assert_eq!(grm.prod_precs[1].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[2].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[3].unwrap(), Precedence{level: 2, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[4].unwrap(), Precedence{level: 3, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[5].unwrap(), Precedence{level: 4, kind: AssocKind::Nonassoc});
        assert!(grm.prod_precs[6].is_none());
        assert_eq!(grm.prod_precs[7], None);
    }

    #[test]
    fn test_prec_override() {
        let grm = yacc_grm(YaccKind::Original, "
            %start expr
            %left '+' '-'
            %left '*' '/'
            %%
            expr : expr '+' expr
                 | expr '-' expr
                 | expr '*' expr
                 | expr '/' expr
                 | '-'  expr %prec '*'
                 | 'id' ;
        ").unwrap();
        assert_eq!(grm.prod_precs.len(), 7);
        assert_eq!(grm.prod_precs[0].unwrap(), Precedence{level: 0, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[1].unwrap(), Precedence{level: 0, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[2].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[3].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert_eq!(grm.prod_precs[4].unwrap(), Precedence{level: 1, kind: AssocKind::Left});
        assert!(grm.prod_precs[5].is_none());
        assert_eq!(grm.prod_precs[6], None);
    }

    #[test]
    fn test_implicit_tokens_rewrite() {
        let grm = yacc_grm(YaccKind::Eco, "
          %implicit_tokens ws1 ws2
          %start S
          %%
          S: 'a' | T;
          T: 'c' |;
          ").unwrap();

        // Check that the above grammar has been rewritten to:
        //   ^ : ^~;
        //   ^~: ~ S;
        //   ~ : ws1 | ws2 | ;
        //   S : 'a' ~ | T;
        //   T : 'c' ~ | ;

        assert_eq!(grm.prod_precs.len(), 9);

        let itfs_rule_idx = grm.nonterm_idx(IMPLICIT_START_NONTERM).unwrap();
        assert_eq!(grm.rules_prods[usize::from(itfs_rule_idx)].len(), 1);

        let itfs_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(itfs_rule_idx)][0])];
        assert_eq!(itfs_prod1.len(), 2);
        assert_eq!(itfs_prod1[0], Symbol::Nonterm(grm.nonterm_idx(IMPLICIT_NONTERM).unwrap()));
        assert_eq!(itfs_prod1[1], Symbol::Nonterm(grm.nonterm_idx(&"S").unwrap()));

        let s_rule_idx = grm.nonterm_idx(&"S").unwrap();
        assert_eq!(grm.rules_prods[usize::from(s_rule_idx)].len(), 2);

        let s_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(s_rule_idx)][0])];
        assert_eq!(s_prod1.len(), 2);
        assert_eq!(s_prod1[0], Symbol::Term(grm.term_idx("a").unwrap()));
        assert_eq!(s_prod1[1], Symbol::Nonterm(grm.nonterm_idx(IMPLICIT_NONTERM).unwrap()));

        let s_prod2 = &grm.prods[usize::from(grm.rules_prods[usize::from(s_rule_idx)][1])];
        assert_eq!(s_prod2.len(), 1);
        assert_eq!(s_prod2[0], Symbol::Nonterm(grm.nonterm_idx("T").unwrap()));

        let t_rule_idx = grm.nonterm_idx(&"T").unwrap();
        assert_eq!(grm.rules_prods[usize::from(s_rule_idx)].len(), 2);

        let t_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(t_rule_idx)][0])];
        assert_eq!(t_prod1.len(), 2);
        assert_eq!(t_prod1[0], Symbol::Term(grm.term_idx("c").unwrap()));
        assert_eq!(t_prod1[1], Symbol::Nonterm(grm.nonterm_idx(IMPLICIT_NONTERM).unwrap()));

        let t_prod2 = &grm.prods[usize::from(grm.rules_prods[usize::from(t_rule_idx)][1])];
        assert_eq!(t_prod2.len(), 0);

        assert_eq!(Some(grm.nonterm_idx(IMPLICIT_NONTERM).unwrap()), grm.implicit_nonterm());
        let i_rule_idx = grm.nonterm_idx(IMPLICIT_NONTERM).unwrap();
        assert_eq!(grm.rules_prods[usize::from(i_rule_idx)].len(), 3);
        let i_prod1 = &grm.prods[usize::from(grm.rules_prods[usize::from(i_rule_idx)][0])];
        let i_prod2 = &grm.prods[usize::from(grm.rules_prods[usize::from(i_rule_idx)][1])];
        assert_eq!(i_prod1.len(), 2);
        assert_eq!(i_prod2.len(), 2);
        // We don't know what order the implicit nonterminal will contain our tokens in,
        // hence the awkward dance below.
        let cnd1 = vec![Symbol::Term(grm.term_idx("ws1").unwrap()),
                        Symbol::Nonterm(grm.implicit_nonterm().unwrap())];
        let cnd2 = vec![Symbol::Term(grm.term_idx("ws2").unwrap()),
                        Symbol::Nonterm(grm.implicit_nonterm().unwrap())];
        assert!((*i_prod1 == cnd1 && *i_prod2 == cnd2) || (*i_prod1 == cnd2 && *i_prod2 == cnd1));
        let i_prod3 = &grm.prods[usize::from(grm.rules_prods[usize::from(i_rule_idx)][2])];
        assert_eq!(i_prod3.len(), 0);
    }

    #[test]
    fn test_has_path() {
        let grm = yacc_grm(YaccKind::Original, "
            %start A
            %%
            A: B;
            B: B 'x' | C;
            C: C 'y' | ;
          ").unwrap();

        let a_nt_idx = grm.nonterm_idx(&"A").unwrap();
        let b_nt_idx = grm.nonterm_idx(&"B").unwrap();
        let c_nt_idx = grm.nonterm_idx(&"C").unwrap();
        assert!(grm.has_path(a_nt_idx, b_nt_idx));
        assert!(grm.has_path(a_nt_idx, c_nt_idx));
        assert!(grm.has_path(b_nt_idx, b_nt_idx));
        assert!(grm.has_path(b_nt_idx, c_nt_idx));
        assert!(grm.has_path(c_nt_idx, c_nt_idx));
        assert!(!grm.has_path(a_nt_idx, a_nt_idx));
        assert!(!grm.has_path(b_nt_idx, a_nt_idx));
        assert!(!grm.has_path(c_nt_idx, a_nt_idx));
    }

    #[test]
    fn test_nonterm_min_costs() {
        let grm = yacc_grm(YaccKind::Original, "
            %start A
            %%
            A: A B | ;
            B: C | D | E;
            C: 'x' B | 'x';
            D: 'y' B | 'y' 'z';
            E: 'x' A | 'x' 'y';
          ").unwrap();

        let scores = nonterm_min_costs(&grm, &vec![1, 1, 1]);
        assert_eq!(scores[usize::from(grm.nonterm_idx(&"A").unwrap())], 0);
        assert_eq!(scores[usize::from(grm.nonterm_idx(&"B").unwrap())], 1);
        assert_eq!(scores[usize::from(grm.nonterm_idx(&"C").unwrap())], 1);
        assert_eq!(scores[usize::from(grm.nonterm_idx(&"D").unwrap())], 2);
        assert_eq!(scores[usize::from(grm.nonterm_idx(&"E").unwrap())], 1);
    }

    #[test]
    fn test_min_sentences() {
        let grm = yacc_grm(YaccKind::Original, "
            %start A
            %%
            A: A B | ;
            B: C | D;
            C: 'x' B | 'x';
            D: 'y' B | 'y' 'z';
          ").unwrap();

        let sg = grm.sentence_generator(|_| 1);

        let find = |nt_name: &str, str_cnds: Vec<Vec<&str>>| {
            let cnds = str_cnds.iter()
                               .map(|x| x.iter()
                                         .map(|y| grm.term_idx(y)
                                                     .unwrap())
                                         .collect::<Vec<TIdx>>())
                               .collect::<Vec<Vec<TIdx>>>();

            let ms = sg.min_sentence(grm.nonterm_idx(nt_name).unwrap());
            if !cnds.iter().any(|x| x == &ms) {
                panic!("{:?} doesn't have any matches in {:?}", ms, str_cnds);
            }

            let min_sts = sg.min_sentences(grm.nonterm_idx(nt_name).unwrap());
            assert_eq!(cnds.len(), min_sts.len());
            for ms in min_sts {
                if !cnds.iter().any(|x| x == &ms) {
                    panic!("{:?} doesn't have any matches in {:?}", ms, str_cnds);
                }
            }
        };

        find("A", vec![vec![]]);
        find("B", vec![vec!["x"]]);
        find("C", vec![vec!["x"]]);
        find("D", vec![vec!["y", "x"], vec!["y", "z"]]);
    }

    #[test]
    fn test_nonterm_max_costs1() {
        let grm = yacc_grm(YaccKind::Original, "
            %start A
            %%
            A: A B | ;
            B: C | D | E;
            C: 'x' B | 'x';
            D: 'y' B | 'y' 'z';
            E: 'x' A | 'x' 'y';
          ").unwrap();

        let scores = nonterm_max_costs(&grm, &vec![1, 1, 1]);
        assert_eq!(scores[usize::from(grm.nonterm_idx("A").unwrap())], u32::max_value());
        assert_eq!(scores[usize::from(grm.nonterm_idx("B").unwrap())], u32::max_value());
        assert_eq!(scores[usize::from(grm.nonterm_idx("C").unwrap())], u32::max_value());
        assert_eq!(scores[usize::from(grm.nonterm_idx("D").unwrap())], u32::max_value());
        assert_eq!(scores[usize::from(grm.nonterm_idx("E").unwrap())], u32::max_value());
    }

    #[test]
    fn test_nonterm_max_costs2() {
        let grm = yacc_grm(YaccKind::Original, "
            %start A
            %%
            A: A B | B;
            B: C | D;
            C: 'x' 'y' | 'x';
            D: 'y' 'x' | 'y' 'x' 'z';
          ").unwrap();

        let scores = nonterm_max_costs(&grm, &vec![1, 1, 1]);
        assert_eq!(scores[usize::from(grm.nonterm_idx("A").unwrap())], u32::max_value());
        assert_eq!(scores[usize::from(grm.nonterm_idx("B").unwrap())], 3);
        assert_eq!(scores[usize::from(grm.nonterm_idx("C").unwrap())], 2);
        assert_eq!(scores[usize::from(grm.nonterm_idx("D").unwrap())], 3);
    }

    #[test]
    fn test_out_of_order_productions() {
        // Example taken from p54 of Locally least-cost error repair in LR parsers, Carl Cerecke
        let grm = yacc_grm(YaccKind::Original, "
            %start S
            %%
            S: A 'c' 'd'
             | B 'c' 'e';
            A: 'a';
            B: 'a'
             | 'b';
            A: 'b';
            ").unwrap();

        assert_eq!(grm.prods_rules, vec![NTIdx::from(1 as u32),
                                         NTIdx::from(1 as u32),
                                         NTIdx::from(2 as u32),
                                         NTIdx::from(3 as u32),
                                         NTIdx::from(3 as u32),
                                         NTIdx::from(2 as u32),
                                         NTIdx::from(0 as u32)]);
    }
}
