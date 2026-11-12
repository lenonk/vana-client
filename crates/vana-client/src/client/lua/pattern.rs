//! Lua 5.4's string patterns (lstrlib.c) in Rust, for `string.find` / `match` / `gmatch` / `gsub`: C's matcher runs
//! no VM instructions, out of the budget hook's reach; this one follows it step for step and checks the deadline.

use mlua::{Lua, MultiValue, Value};
use std::time::Instant;

const MAXCAPTURES: usize = 32;
/// lstrlib's MAXCCALLS: how deep the matcher may recurse.
const MAXCCALLS: usize = 200;
const ESC: u8 = b'%';
const SPECIALS: &[u8] = b"^$*+?.([%-";
/// Units of work between deadline checks: a match step, a pattern byte scanned, an output push, 256 bytes compared or
/// written.
const CHECK_EVERY: usize = 4096;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cap {
    Unfinished,
    Position,
    Len(usize),
}

struct State<'a> {
    src: &'a [u8],
    pat: &'a [u8],
    level: usize,
    capture: [(usize, Cap); MAXCAPTURES],
    depth: usize,
    work: usize,
    deadline: Option<Instant>,
}

/// How a call fails: our error (raised at the caller's line, as luaL_error), a bad argument (luaL_argerror: it
/// names the function as called), or one passed on as it came (a replacement function's error, memory).
enum Fail {
    Raise(String),
    Arg(usize, String),
    Pass(String),
}

impl From<mlua::Error> for Fail {
    fn from(e: mlua::Error) -> Self {
        Fail::Pass(message(&e))
    }
}

/// An error's text as Lua would hold it (mlua adds a traceback to one it caught).
fn message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::RuntimeError(m) => m.split_once("\nstack traceback:\n").map_or(m.as_str(), |x| x.0).to_string(),
        mlua::Error::MemoryError(_) => "not enough memory".into(),
        mlua::Error::CallbackError { cause, .. } | mlua::Error::WithContext { cause, .. } => message(cause),
        e => e.to_string(),
    }
}

type R<T> = Result<T, Fail>;

fn err(msg: impl Into<String>) -> Fail {
    Fail::Raise(msg.into())
}

impl<'a> State<'a> {
    fn new(src: &'a [u8], pat: &'a [u8], deadline: Option<Instant>) -> Self {
        Self { src, pat, level: 0, capture: [(0, Cap::Unfinished); MAXCAPTURES], depth: MAXCCALLS, work: 0, deadline }
    }

    /// Before each match attempt (reprepstate).
    fn reset(&mut self) {
        self.level = 0;
        self.depth = MAXCCALLS;
    }

    /// One unit of work.
    fn step(&mut self) -> R<()> {
        self.charge(1)
    }

    /// `n` units of work; past the deadline the call stops as the VM's hook would stop it.
    fn charge(&mut self, n: usize) -> R<()> {
        self.work += n;
        if self.work >= CHECK_EVERY {
            self.work = 0;
            if self.deadline.is_some_and(|d| Instant::now() > d) {
                return Err(err("ran past its time budget"));
            }
        }
        Ok(())
    }

    /// The subject's byte at `s`, 0 past its end (C reads the terminating NUL there).
    fn s_at(&self, s: usize) -> u8 {
        self.src.get(s).copied().unwrap_or(0)
    }

    fn p_at(&self, p: usize) -> u8 {
        self.pat.get(p).copied().unwrap_or(0)
    }

    /// Where the class at `p` ends; a set's scan is charged by its length.
    fn class_end(&mut self, p: usize) -> R<usize> {
        let ep = self.class_end_at(p)?;
        self.charge(ep - p)?;
        Ok(ep)
    }

    fn class_end_at(&self, mut p: usize) -> R<usize> {
        let c = self.p_at(p);
        p += 1;
        match c {
            ESC => {
                if p >= self.pat.len() {
                    return Err(err("malformed pattern (ends with '%')"));
                }
                Ok(p + 1)
            }
            b'[' => {
                if self.p_at(p) == b'^' {
                    p += 1;
                }
                // Look for a ']' (the first one may be part of the set, as in "[]]").
                loop {
                    if p >= self.pat.len() {
                        return Err(err("malformed pattern (missing ']')"));
                    }
                    let c = self.pat[p];
                    p += 1;
                    if c == ESC && p < self.pat.len() {
                        p += 1;
                    }
                    if self.p_at(p) == b']' {
                        break;
                    }
                }
                Ok(p + 1)
            }
            _ => Ok(p),
        }
    }

    /// `[set]` from `p` (at '[') to `ec` (at its ']').
    fn bracket(&self, c: u8, mut p: usize, ec: usize) -> bool {
        let mut sig = true;
        if self.p_at(p + 1) == b'^' {
            sig = false;
            p += 1;
        }
        loop {
            p += 1;
            if p >= ec {
                break;
            }
            if self.pat[p] == ESC {
                p += 1;
                if class(c, self.p_at(p)) {
                    return sig;
                }
            } else if self.p_at(p + 1) == b'-' && p + 2 < ec {
                p += 2;
                if self.pat[p - 2] <= c && c <= self.pat[p] {
                    return sig;
                }
            } else if self.pat[p] == c {
                return sig;
            }
        }
        !sig
    }

    /// Whether the class `pat[p..ep]` matches `src[s]`; a set's scan is charged by its length.
    fn single(&mut self, s: usize, p: usize, ep: usize) -> R<bool> {
        if s >= self.src.len() {
            return Ok(false);
        }
        let c = self.src[s];
        Ok(match self.pat[p] {
            b'.' => true,
            ESC => class(c, self.p_at(p + 1)),
            b'[' => {
                self.charge(ep - p)?;
                self.bracket(c, p, ep - 1)
            }
            pc => pc == c,
        })
    }

    fn balance(&mut self, s: usize, p: usize) -> R<Option<usize>> {
        if p + 1 >= self.pat.len() {
            return Err(err("malformed pattern (missing arguments to '%b')"));
        }
        if self.s_at(s) != self.pat[p] {
            return Ok(None);
        }
        let (b, e) = (self.pat[p], self.pat[p + 1]);
        let mut cont = 1;
        let mut s = s + 1;
        while s < self.src.len() {
            self.step()?;
            let c = self.src[s];
            if c == e {
                cont -= 1;
                if cont == 0 {
                    return Ok(Some(s + 1));
                }
            } else if c == b {
                cont += 1;
            }
            s += 1;
        }
        Ok(None)
    }

    fn max_expand(&mut self, s: usize, p: usize, ep: usize) -> R<Option<usize>> {
        let mut i = 0;
        while self.single(s + i, p, ep)? {
            self.step()?;
            i += 1;
        }
        // Try with the most repetitions first, then one fewer each time.
        loop {
            if let Some(r) = self.do_match(s + i, ep + 1)? {
                return Ok(Some(r));
            }
            if i == 0 {
                return Ok(None);
            }
            i -= 1;
        }
    }

    fn min_expand(&mut self, mut s: usize, p: usize, ep: usize) -> R<Option<usize>> {
        loop {
            if let Some(r) = self.do_match(s, ep + 1)? {
                return Ok(Some(r));
            }
            if self.single(s, p, ep)? {
                s += 1;
            } else {
                return Ok(None);
            }
        }
    }

    fn start_capture(&mut self, s: usize, p: usize, what: Cap) -> R<Option<usize>> {
        if self.level >= MAXCAPTURES {
            return Err(err("too many captures"));
        }
        self.capture[self.level] = (s, what);
        self.level += 1;
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.level -= 1;
        }
        Ok(r)
    }

    fn end_capture(&mut self, s: usize, p: usize) -> R<Option<usize>> {
        let l = (0..self.level)
            .rev()
            .find(|&l| self.capture[l].1 == Cap::Unfinished)
            .ok_or_else(|| err("invalid pattern capture"))?;
        self.capture[l].1 = Cap::Len(s - self.capture[l].0);
        let r = self.do_match(s, p)?;
        if r.is_none() {
            self.capture[l].1 = Cap::Unfinished;
        }
        Ok(r)
    }

    fn check_capture(&self, l: u8) -> R<usize> {
        let l = l as i32 - b'1' as i32;
        if l < 0 || l as usize >= self.level || self.capture[l as usize].1 == Cap::Unfinished {
            return Err(err(format!("invalid capture index %{}", l + 1)));
        }
        Ok(l as usize)
    }

    fn match_capture(&mut self, s: usize, l: u8) -> R<Option<usize>> {
        let l = self.check_capture(l)?;
        let (init, cap) = self.capture[l];
        // A position capture has no text: C compares (size_t)-2 bytes, which never fits.
        let Cap::Len(len) = cap else { return Ok(None) };
        self.charge(len / 256)?;
        if self.src.len() - s >= len && self.src[init..init + len] == self.src[s..s + len] {
            Ok(Some(s + len))
        } else {
            Ok(None)
        }
    }

    /// lstrlib's `match`: where the match of `pat[p..]` at `src[s..]` ends, if it matches.
    fn do_match(&mut self, s: usize, p: usize) -> R<Option<usize>> {
        if self.depth == 0 {
            return Err(err("pattern too complex"));
        }
        self.depth -= 1;
        let r = self.match_body(s, p);
        self.depth += 1;
        r
    }

    fn match_body(&mut self, mut s: usize, mut p: usize) -> R<Option<usize>> {
        loop {
            self.step()?;
            if p >= self.pat.len() {
                return Ok(Some(s));
            }
            match self.pat[p] {
                b'(' => {
                    return if self.p_at(p + 1) == b')' {
                        self.start_capture(s, p + 2, Cap::Position)
                    } else {
                        self.start_capture(s, p + 1, Cap::Unfinished)
                    };
                }
                b')' => return self.end_capture(s, p + 1),
                b'$' if p + 1 == self.pat.len() => return Ok((s == self.src.len()).then_some(s)),
                ESC if self.p_at(p + 1) == b'b' => match self.balance(s, p + 2)? {
                    Some(e) => {
                        s = e;
                        p += 4;
                        continue;
                    }
                    None => return Ok(None),
                },
                ESC if self.p_at(p + 1) == b'f' => {
                    p += 2;
                    if self.p_at(p) != b'[' {
                        return Err(err("missing '[' after '%f' in pattern"));
                    }
                    let ep = self.class_end(p)?;
                    let previous = if s == 0 { 0 } else { self.src[s - 1] };
                    self.charge(2 * (ep - p))?;
                    if !self.bracket(previous, p, ep - 1) && self.bracket(self.s_at(s), p, ep - 1) {
                        p = ep;
                        continue;
                    }
                    return Ok(None);
                }
                ESC if self.p_at(p + 1).is_ascii_digit() => match self.match_capture(s, self.pat[p + 1])? {
                    Some(e) => {
                        s = e;
                        p += 2;
                        continue;
                    }
                    None => return Ok(None),
                },
                _ => {
                    let ep = self.class_end(p)?;
                    let q = self.p_at(ep);
                    if !self.single(s, p, ep)? {
                        // Accept empty?
                        if matches!(q, b'*' | b'?' | b'-') && ep < self.pat.len() {
                            p = ep + 1;
                            continue;
                        }
                        return Ok(None);
                    }
                    match q {
                        b'?' if ep < self.pat.len() => {
                            if let Some(r) = self.do_match(s + 1, ep + 1)? {
                                return Ok(Some(r));
                            }
                            p = ep + 1;
                        }
                        b'+' if ep < self.pat.len() => return self.max_expand(s + 1, p, ep),
                        b'*' if ep < self.pat.len() => return self.max_expand(s, p, ep),
                        b'-' if ep < self.pat.len() => return self.min_expand(s, p, ep),
                        _ => {
                            s += 1;
                            p = ep;
                        }
                    }
                }
            }
        }
    }

    /// Capture `i` (the whole match `s..e` when there are none), as lstrlib's push_onecapture.
    fn capture_value(&self, lua: &Lua, i: usize, s: usize, e: usize) -> R<Value> {
        if i >= self.level {
            if i != 0 {
                return Err(err(format!("invalid capture index %{}", i + 1)));
            }
            return Ok(Value::String(lua.create_string(&self.src[s..e])?));
        }
        let (init, cap) = self.capture[i];
        match cap {
            Cap::Unfinished => Err(err("unfinished capture")),
            Cap::Position => Ok(Value::Integer(init as i64 + 1)),
            Cap::Len(l) => Ok(Value::String(lua.create_string(&self.src[init..init + l])?)),
        }
    }

    /// Every capture (or the whole match when there are none); with no match span, only the captures.
    fn captures(&self, lua: &Lua, span: Option<(usize, usize)>) -> R<MultiValue> {
        let n = if self.level == 0 && span.is_some() { 1 } else { self.level };
        let (s, e) = span.unwrap_or((0, 0));
        (0..n).map(|i| self.capture_value(lua, i, s, e)).collect()
    }
}

/// The C library's character classes (the "C" locale: ASCII only).
fn class(c: u8, cl: u8) -> bool {
    let res = match cl.to_ascii_lowercase() {
        b'a' => c.is_ascii_alphabetic(),
        b'c' => c.is_ascii_control(),
        b'd' => c.is_ascii_digit(),
        b'g' => c.is_ascii_graphic(),
        b'l' => c.is_ascii_lowercase(),
        b'p' => c.is_ascii_punctuation(),
        b's' => matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'),
        b'u' => c.is_ascii_uppercase(),
        b'w' => c.is_ascii_alphanumeric(),
        b'x' => c.is_ascii_hexdigit(),
        // Deprecated in 5.2, still in 5.4's lstrlib.
        b'z' => c == 0,
        _ => return cl == c,
    };
    if cl.is_ascii_uppercase() { !res } else { res }
}

/// lstrlib's posrelatI: a 1-based start position, negative from the end, clipped to 1.
fn posrelat(pos: i64, len: usize) -> usize {
    if pos > 0 {
        pos as usize
    } else if pos == 0 || pos < -(len as i64) {
        1
    } else {
        (len as i64 + pos + 1) as usize
    }
}

/// The arguments as C sees them: a missing one is "no value", not nil.
struct Args(Vec<Value>);

impl Args {
    fn get(&self, n: usize) -> &Value {
        self.0.get(n - 1).unwrap_or(&Value::Nil)
    }

    /// luaL_typeerror's name for the argument.
    fn type_name(&self, n: usize) -> String {
        match self.0.get(n - 1) {
            None => "no value".into(),
            Some(Value::LightUserData(_)) => "light userdata".into(),
            Some(v) => {
                let named = match v {
                    Value::Table(t) => t.metatable().and_then(|m| m.raw_get::<Value>("__name").ok()),
                    Value::UserData(u) => u.metatable().ok().and_then(|m| m.get::<Value>("__name").ok()),
                    _ => None,
                };
                match named {
                    Some(Value::String(n)) => n.to_string_lossy(),
                    _ => v.type_name().to_string(),
                }
            }
        }
    }

    fn bad(&self, n: usize, why: &str) -> Fail {
        Fail::Arg(n, why.to_string())
    }

    /// luaL_checklstring: a string, or a number as its text.
    fn str(&self, lua: &Lua, n: usize) -> R<mlua::LuaString> {
        let v = self.get(n);
        let s = if matches!(v, Value::String(_) | Value::Integer(_) | Value::Number(_)) {
            lua.coerce_string(v.clone())?
        } else {
            None
        };
        s.ok_or_else(|| self.bad(n, &format!("string expected, got {}", self.type_name(n))))
    }

    /// luaL_optinteger.
    fn int(&self, lua: &Lua, n: usize, default: i64) -> R<i64> {
        let v = self.get(n);
        if v.is_nil() {
            return Ok(default);
        }
        if let Some(i) = lua.coerce_integer(v.clone())? {
            return Ok(i);
        }
        let why = if lua.coerce_number(v.clone())?.is_some() {
            "number has no integer representation".to_string()
        } else {
            format!("number expected, got {}", self.type_name(n))
        };
        Err(self.bad(n, &why))
    }
}

fn deadline(lua: &Lua) -> Option<Instant> {
    lua.app_data_ref::<std::rc::Rc<std::cell::RefCell<super::vm::ModState>>>().and_then(|st| st.borrow().deadline)
}

fn find_aux(lua: &Lua, a: &Args, find: bool) -> R<MultiValue> {
    let s = a.str(lua, 1)?;
    let p = a.str(lua, 2)?;
    let (s, p) = (s.as_bytes(), p.as_bytes());
    let (s, p): (&[u8], &[u8]) = (&s, &p);
    let init = posrelat(a.int(lua, 3, 1)?, s.len()) - 1;
    if init > s.len() {
        return Ok(MultiValue::from_vec(vec![Value::Nil]));
    }
    let plain = !matches!(a.get(4), Value::Nil | Value::Boolean(false));
    if find && (plain || !p.iter().any(|c| SPECIALS.contains(c))) {
        // A plain search (linear, so no budget needed).
        return Ok(match memchr::memmem::find(&s[init..], p) {
            Some(k) => MultiValue::from_vec(vec![
                Value::Integer((init + k + 1) as i64),
                Value::Integer((init + k + p.len()) as i64),
            ]),
            None => MultiValue::from_vec(vec![Value::Nil]),
        });
    }
    let anchor = p.first() == Some(&b'^');
    let p = if anchor { &p[1..] } else { p };
    let mut ms = State::new(s, p, deadline(lua));
    let mut s1 = init;
    loop {
        ms.reset();
        if let Some(e) = ms.do_match(s1, 0)? {
            return if find {
                let mut out = vec![Value::Integer(s1 as i64 + 1), Value::Integer(e as i64)];
                out.extend(ms.captures(lua, None)?);
                Ok(MultiValue::from_vec(out))
            } else {
                ms.captures(lua, Some((s1, e)))
            };
        }
        s1 += 1;
        if s1 > s.len() || anchor {
            break;
        }
    }
    Ok(MultiValue::from_vec(vec![Value::Nil]))
}

/// Marks a failed call's results for `raise`.
static FAILED: u8 = 0;

fn failed() -> Value {
    Value::LightUserData(mlua::LightUserData(failed_ptr()))
}

/// A call's results; a failure as (FAILED, level, message).
/// A call's results; a failure as (FAILED, kind, message, argument) for `raise`.
fn results(lua: &Lua, r: R<MultiValue>) -> mlua::Result<MultiValue> {
    let (kind, m, arg) = match r {
        Ok(v) => return Ok(v),
        Err(Fail::Pass(m)) => (0, m, 0),
        Err(Fail::Raise(m)) => (1, m, 0),
        Err(Fail::Arg(n, m)) => (2, m, n as i64),
    };
    Ok(MultiValue::from_vec(vec![
        failed(),
        Value::Integer(kind),
        Value::String(lua.create_string(m)?),
        Value::Integer(arg),
    ]))
}

/// The C function a mod calls: its Rust half (upvalue 1) returns a failure as values, raised here as luaL_error
/// raises one, a string at the caller's line. A C function keeps that line under a tail call; a Lua one would not.
unsafe extern "C-unwind" fn raise(l: *mut mlua::ffi::lua_State) -> std::ffi::c_int {
    use mlua::ffi;
    // SAFETY: called by Lua with a valid state; nothing here owns memory a longjmp could skip.
    unsafe {
        let n = ffi::lua_gettop(l);
        ffi::lua_pushvalue(l, ffi::lua_upvalueindex(1));
        ffi::lua_insert(l, 1);
        ffi::lua_call(l, n, ffi::LUA_MULTRET);
        let got = ffi::lua_gettop(l);
        if got == 4 && ffi::lua_touserdata(l, 1) == failed_ptr() {
            match ffi::lua_tointeger(l, 2) {
                2 => {
                    ffi::luaL_argerror(l, ffi::lua_tointeger(l, 4) as std::ffi::c_int, ffi::lua_tostring(l, 3));
                }
                kind => {
                    ffi::lua_settop(l, 3);
                    if kind == 1 {
                        ffi::luaL_where(l, 1);
                        ffi::lua_insert(l, 3);
                        ffi::lua_concat(l, 2);
                    }
                }
            }
            ffi::lua_error(l);
        }
        got
    }
}

/// gmatch: as raise, and the iterator it returns is wrapped the same way.
unsafe extern "C-unwind" fn raise_gmatch(l: *mut mlua::ffi::lua_State) -> std::ffi::c_int {
    // SAFETY: as raise; on success the stack holds the iterator alone.
    unsafe {
        raise(l);
        mlua::ffi::lua_pushcclosure(l, raise, 1);
        1
    }
}

fn failed_ptr() -> *mut std::ffi::c_void {
    &FAILED as *const u8 as *mut std::ffi::c_void
}

/// Replaces `string.find`, `match`, `gmatch` and `gsub` in the mod's string table (also its strings' methods).
pub(super) fn install(lua: &Lua, string: &mlua::Table) -> mlua::Result<()> {
    let find = lua.create_function(|lua, a: MultiValue| results(lua, find_aux(lua, &Args(a.into_vec()), true)))?;
    let r#match = lua.create_function(|lua, a: MultiValue| results(lua, find_aux(lua, &Args(a.into_vec()), false)))?;
    let gmatch = lua.create_function(|lua, a: MultiValue| results(lua, gmatch(lua, &Args(a.into_vec()))))?;
    let gsub = lua.create_function(|lua, a: MultiValue| {
        results(lua, gsub(lua, &Args(a.into_vec())).map(|(s, n)| MultiValue::from_vec(vec![s, Value::Integer(n)])))
    })?;
    // SAFETY: the stack holds the five arguments; each closure takes one of them as its upvalue.
    unsafe {
        lua.exec_raw::<()>((string.clone(), find, r#match, gmatch, gsub), |l| {
            use mlua::ffi;
            for (i, name, f) in [
                (2, c"find", raise as ffi::lua_CFunction),
                (3, c"match", raise),
                (4, c"gmatch", raise_gmatch),
                (5, c"gsub", raise),
            ] {
                ffi::lua_pushvalue(l, i);
                ffi::lua_pushcclosure(l, f, 1);
                ffi::lua_setfield(l, 1, name.as_ptr());
            }
            ffi::lua_settop(l, 0);
        })
    }
}

/// gmatch's iterator, over the subject and pattern as Lua holds them.
fn gmatch(lua: &Lua, a: &Args) -> R<MultiValue> {
    let s = a.str(lua, 1)?;
    let p = a.str(lua, 2)?;
    let len = s.as_bytes().len();
    let mut src = (posrelat(a.int(lua, 3, 1)?, len) - 1).min(len + 1);
    let mut last: Option<usize> = None;
    let it = lua.create_function_mut(move |lua, ()| {
        let (sb, pb) = (s.as_bytes(), p.as_bytes());
        let r = (|| {
            let mut ms = State::new(&sb, &pb, deadline(lua));
            while src <= len {
                ms.reset();
                if let Some(e) = ms.do_match(src, 0)?
                    && Some(e) != last
                {
                    let start = src;
                    src = e;
                    last = Some(e);
                    return ms.captures(lua, Some((start, e)));
                }
                src += 1;
            }
            Ok(MultiValue::new())
        })();
        results(lua, r)
    })?;
    Ok(MultiValue::from_vec(vec![Value::Function(it)]))
}

fn gsub(lua: &Lua, a: &Args) -> R<(Value, i64)> {
    let src = a.str(lua, 1)?;
    let p = a.str(lua, 2)?;
    // As C: the count (4) is read before the replacement's type (3) is checked.
    let max_s = a.int(lua, 4, src.as_bytes().len() as i64 + 1)?;
    let repl = a.get(3).clone();
    if !matches!(repl, Value::Integer(_) | Value::Number(_) | Value::String(_) | Value::Table(_) | Value::Function(_)) {
        return Err(a.bad(3, &format!("string/function/table expected, got {}", a.type_name(3))));
    }
    let (src_b, p_b) = (src.as_bytes(), p.as_bytes());
    let (s, p): (&[u8], &[u8]) = (&src_b, &p_b);
    // A number replacement is its string form.
    let repl = match repl {
        Value::Integer(_) | Value::Number(_) => {
            Value::String(lua.coerce_string(repl)?.ok_or_else(|| err("bad replacement"))?)
        }
        r => r,
    };
    let anchor = p.first() == Some(&b'^');
    let p = if anchor { &p[1..] } else { p };
    let mut ms = State::new(s, p, deadline(lua));
    // Lua's buffer counts against the mod's memory limit; ours is Rust's, so it is held to what is left of it.
    let mut out = Out { buf: Vec::new(), cap: super::vm::MEMORY.saturating_sub(lua.used_memory()) };
    let (mut at, mut last, mut n, mut changed) = (0usize, None::<usize>, 0i64, false);
    while n < max_s {
        ms.reset();
        match ms.do_match(at, 0)? {
            Some(e) if Some(e) != last => {
                n += 1;
                changed |= add_value(lua, &mut ms, &mut out, at, e, &repl)?;
                at = e;
                last = Some(e);
            }
            _ if at < s.len() => {
                out.push(&mut ms, &s[at..at + 1])?;
                at += 1;
            }
            _ => break,
        }
        if anchor {
            break;
        }
    }
    if !changed {
        // As C, which converted the argument in place: a number subject comes back as its string.
        return Ok((Value::String(src.clone()), n));
    }
    out.push(&mut ms, &s[at..])?;
    Ok((Value::String(lua.create_string(&out.buf)?), n))
}

/// gsub's result as it grows: held to the mod's memory, its bytes charged as work.
struct Out {
    buf: Vec<u8>,
    cap: usize,
}

impl Out {
    fn push(&mut self, ms: &mut State, bytes: &[u8]) -> R<()> {
        if self.buf.len() + bytes.len() > self.cap {
            return Err(Fail::Pass("not enough memory".into()));
        }
        self.buf.extend_from_slice(bytes);
        ms.charge(1 + bytes.len() / 256)
    }
}

/// One replacement (lstrlib's add_value): whether it changed the text.
fn add_value(lua: &Lua, ms: &mut State, out: &mut Out, s: usize, e: usize, repl: &Value) -> R<bool> {
    let src = ms.src;
    let v = match repl {
        Value::String(r) => {
            let r = r.as_bytes();
            let mut i = 0;
            while i < r.len() {
                // The text up to the next escape in one piece.
                let k = r[i..].iter().position(|&c| c == ESC).map_or(r.len(), |k| i + k);
                out.push(ms, &r[i..k])?;
                if k == r.len() {
                    break;
                }
                let d = r.get(k + 1).copied().unwrap_or(0);
                i = k + 2;
                if d == ESC {
                    out.push(ms, &[ESC])?;
                } else if d == b'0' {
                    out.push(ms, &src[s..e])?;
                } else if d.is_ascii_digit() {
                    match ms.capture_value(lua, (d - b'1') as usize, s, e)? {
                        Value::String(t) => out.push(ms, &t.as_bytes())?,
                        Value::Integer(k) => out.push(ms, k.to_string().as_bytes())?,
                        _ => {}
                    }
                } else {
                    return Err(err("invalid use of '%' in replacement string"));
                }
            }
            return Ok(true);
        }
        Value::Function(f) => f.call::<Value>(ms.captures(lua, Some((s, e)))?)?,
        Value::Table(t) => t.get::<Value>(ms.capture_value(lua, 0, s, e)?)?,
        _ => Value::Nil,
    };
    match v {
        Value::Nil | Value::Boolean(false) => {
            out.push(ms, &src[s..e])?;
            Ok(false)
        }
        Value::String(t) => {
            out.push(ms, &t.as_bytes())?;
            Ok(true)
        }
        v @ (Value::Integer(_) | Value::Number(_)) => {
            let t = lua.coerce_string(v)?.ok_or_else(|| err("invalid replacement value"))?;
            out.push(ms, &t.as_bytes())?;
            Ok(true)
        }
        v => Err(err(format!("invalid replacement value (a {})", v.type_name()))),
    }
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Value};

    /// The same calls through Lua's string library and through ours give the same results and the same errors.
    #[test]
    fn same_as_lua() {
        let ours = Lua::new();
        super::install(&ours, &ours.globals().get("string").unwrap()).unwrap();
        let theirs = Lua::new();
        const CASES: &[&str] = &[
            "string.find('hello world', 'o w')",
            "string.find('hello world', 'o', 6)",
            "string.find('hello world', 'l+')",
            "string.find('hello', '')",
            "string.find('hello', '', 10)",
            "string.find('hello', '', 6)",
            "string.find('hello', 'l', -2)",
            "string.find('hello', 'l', -20)",
            "string.find('a.b', '.', 1, true)",
            "string.find('a+b', '+', 1, true)",
            "string.find('abc', '[a-', 1, true)",
            "string.find('  key = value', '^%s*(%w+)%s*=%s*(%w+)$')",
            "string.find('[Party] Name: hi there', '^%[(%a+)%] (%a+): (.*)$')",
            "string.find('THE (quick) fox', '%((%a+)%)')",
            "string.find('f(a(b)c)d', '%b()')",
            "string.find('THE (quick) fox', '%f[%a]%a+', 5)",
            "string.find('hello', '()ll()')",
            "string.find('abcabc', '(a)(b)(c)%1%2%3')",
            "string.find('x = 1', '(%w+) = (%d+)')",
            "string.find('aaa', 'a-b')",
            "string.find('aaab', 'a-b')",
            "string.find('aaab', '^a-$')",
            "string.find('abc', '[^a]+')",
            "string.find('a]b', '[]]')",
            "string.find('a-b', '[a%-]+')",
            "string.find('a-b', '[%a-]+', 2)",
            "string.find('0x1F', '%x+', 3)",
            "string.find('\\0ab', '%z')",
            "string.find('a\\0b', '\\0')",
            "string.find('ab', 'a?b')",
            "string.find('b', 'a?b')",
            "string.find('$x$', '$x%$')",
            "string.find('x$', 'x$')",
            "string.find('^x', '^^x')",
            "string.match('2024-11-02', '(%d+)-(%d+)-(%d+)')",
            "string.match('  trim me  ', '^%s*(.-)%s*$')",
            "string.match('key=val', '()=()')",
            "string.match('hello', '.-')",
            "string.match('hello', 'l*', 3)",
            "string.match('PUNCT!?', '%p+')",
            "string.match('A1b2', '%W')",
            "string.match('\\t\\n x', '%S')",
            "string.match('ctrl\\1', '%c')",
            "string.match('Caps', '%u%l+')",
            "string.match('abc', '%g+')",
            "string.match('[x]', '%[(.)%]')",
            "string.match('hello', '(h)(e)(l)(l)(o)')",
            "string.match('^x', '^x')",
            "select('#', string.match('abc', 'z'))",
            "table.concat({string.gsub('hello world', 'o', '0')}, ',')",
            "table.concat({string.gsub('hello world', 'o', '0', 1)}, ',')",
            "table.concat({string.gsub('hello world', '(%w+)', '<%1>')}, ',')",
            "table.concat({string.gsub('hello world', '%w+', '%0 %0')}, ',')",
            "table.concat({string.gsub('abc', '', '-')}, ',')",
            "table.concat({string.gsub('abc', '.-', '-')}, ',')",
            "table.concat({string.gsub('abc', 'b*', '-')}, ',')",
            "table.concat({string.gsub('abc', '^a', 'X')}, ',')",
            "table.concat({string.gsub('aaa', '^a', 'X')}, ',')",
            "table.concat({string.gsub('hello', 'l', {l = 'L'})}, ',')",
            "table.concat({string.gsub('hello', '(l)', {})}, ',')",
            "table.concat({string.gsub('$name is $age', '%$(%w+)', {name = 'Bob', age = 42})}, ',')",
            "table.concat({string.gsub('hello', 'l', function(c) return c:upper() end)}, ',')",
            "table.concat({string.gsub('hello', 'l', function() return false end)}, ',')",
            "table.concat({string.gsub('a,b,c', ',', 5)}, ',')",
            "table.concat({string.gsub('a,b,c', ',', 1.5)}, ',')",
            "table.concat({string.gsub('abc', '()', '%1')}, ',')",
            "table.concat({string.gsub('abc', 'b', '%%')}, ',')",
            "table.concat({string.gsub('abc', 'b', '%%', 0)}, ',')",
            "table.concat({string.gsub('abc', 'b', '%%', -1)}, ',')",
            "table.concat({string.gsub(12345, 3, 'x')}, ',')",
            "math.type((string.gsub(12345, 9, 'x')))",
            "(function() local t = {} for k, v in string.gmatch('a=1, b=2, c=3', '(%w+)=(%w+)') do t[#t+1] = k..v end return table.concat(t, ' ') end)()",
            "(function() local t = {} for w in string.gmatch('one two  three', '%a+') do t[#t+1] = w end return table.concat(t, '|') end)()",
            "(function() local t = {} for w in string.gmatch('abc', '') do t[#t+1] = '.' end return #t end)()",
            "(function() local t = {} for w in string.gmatch('abc', 'b*') do t[#t+1] = '['..w..']' end return table.concat(t) end)()",
            "(function() local t = {} for w in string.gmatch('hello world', '%a+', 3) do t[#t+1] = w end return table.concat(t, '|') end)()",
            "(function() local t = {} for w in string.gmatch('^a^a', '^a') do t[#t+1] = w end return #t end)()",
            "(function() local t = {} for p in string.gmatch('abc', '()') do t[#t+1] = p end return table.concat(t, ',') end)()",
            "('Method Call'):find('C')",
            "('x'):rep(3):gsub('x', 'y')",
            "string.find('<a<b>>c', '%b<>')",
            "string.find('[[x]]', '%b[]')",
            "string.find('abc', '%f[%z]')",
            "string.find('ab cd', '%f[%A]')",
            "string.find('\\200\\201a', '[\\128-\\255]+')",
            "string.match('caf\\195\\169', '%a+')",
            "table.concat({string.gsub('\\255x', '.', '%0%0')}, ',')",
            "(function() local it = string.gmatch('a', 'b') return select('#', it()) end)()",
            // Errors are strings, at the caller's line as C's (none when pcall calls the function itself).
            "(function() local ok, e = pcall(string.find, 'a', '%') return type(e), e end)()",
            "select(2, pcall(function() return string.find('a', '%') end))",
            "select(2, pcall(function() for w in string.gmatch('a', '%') do end end))",
            "select(2, pcall(string.gsub, 'a', 'a', function() error('boom') end))",
            "select(2, pcall(string.find))",
            "select(2, pcall(string.match, 'a'))",
            "select(2, pcall(string.find, 'a', 'a', 1.5))",
            "select(2, pcall(string.find, 'a', 'a', 'x'))",
            "select(2, pcall(string.gsub, 'a', 'a'))",
            "select(2, pcall(string.gsub, 'a', 'a', nil, 'x'))",
            "select(2, pcall(string.gmatch, {}, 'a'))",
            "select(2, pcall(function() return ('x'):find({}) end))",
            "select(2, pcall(function() local s = 'x' return s.find() end))",
            "select(2, pcall(string.find, setmetatable({}, {__name = 'Thing'}), 'a'))",
            // Errors.
            "string.find('a', '%')",
            "string.find('a', '[a')",
            "string.find('a', '[a%')",
            "string.find('a', '(a')",
            "string.match('a', '(a')",
            "string.find('a', 'a)')",
            "string.find('a', '%1')",
            "string.find('aa', '(a)%2')",
            "string.find('a', '%f')",
            "string.find('a', '%b')",
            "string.find('a', '%ba')",
            "string.gsub('a', 'a', '%2')",
            "string.gsub('a', 'a', '%x')",
            "string.gsub('a', 'a', '%')",
            "string.gsub('a', 'a', true)",
            "string.gsub('a', 'a', function() return {} end)",
            "string.find(string.rep('a', 300), string.rep('a?', 300))",
            "string.find('a', string.rep('()', 33))",
            "string.find({}, 'a')",
        ];
        for case in CASES {
            let run = |lua: &Lua| {
                lua.load(format!("return {case}"))
                    .set_name("=case")
                    .eval::<mlua::MultiValue>()
                    .map(|v| v.iter().map(show).collect::<Vec<_>>())
            };
            match (run(&ours), run(&theirs)) {
                (Ok(a), Ok(b)) => assert_eq!(a, b, "{case}"),
                (Err(a), Err(b)) => {
                    let core = core(&b.to_string());
                    assert!(a.to_string().contains(&core), "{case}: ours {a}, Lua's {core}");
                }
                (a, b) => panic!("{case}: ours {a:?}, Lua's {b:?}"),
            }
        }
    }

    /// Random patterns from pattern pieces over random subjects, through both matchers (a fixed seed).
    #[test]
    fn same_as_lua_random() {
        let ours = Lua::new();
        super::install(&ours, &ours.globals().get("string").unwrap()).unwrap();
        let theirs = Lua::new();
        const PIECES: &[&str] = &[
            "a", "b", ".", "%a", "%d", "%s", "%W", "[ab]", "[^a]", "[a-c]", "[%d.]", "*", "+", "-", "?", "(", ")",
            "()", "%1", "%b()", "%f[%a]", "^", "$", "%%", "x",
        ];
        let mut seed = 0x2545f4914f6cdd1du64;
        let mut next = |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n as u64) as usize
        };
        for _ in 0..4000 {
            let pat: String = (0..1 + next(6)).map(|_| PIECES[next(PIECES.len())]).collect();
            let subject: String =
                (0..next(12)).map(|_| ["a", "b", "c", "1", " ", "(", ")", "x", "."][next(9)]).collect();
            let init = next(5) as i64 - 2;
            for f in ["find", "match", "gsub_s", "gsub_f", "gmatch"] {
                let code = match f {
                    "gsub_s" => format!("return string.gsub({subject:?}, {pat:?}, '<%0>')"),
                    "gsub_f" => format!(
                        "return string.gsub({subject:?}, {pat:?}, function(...) return select('#', ...) .. '' end, 3)"
                    ),
                    "gmatch" => format!(
                        "local t = {{}} for a, b in string.gmatch({subject:?}, {pat:?}, {init}) do t[#t+1] = tostring(a) .. '/' .. tostring(b) end return table.concat(t, ',')"
                    ),
                    _ => format!("return string.{f}({subject:?}, {pat:?}, {init})"),
                };
                let run = |lua: &Lua| {
                    lua.load(code.as_str())
                        .set_name("=case")
                        .eval::<mlua::MultiValue>()
                        .map(|v| v.iter().map(show).collect::<Vec<_>>())
                };
                match (run(&ours), run(&theirs)) {
                    (Ok(a), Ok(b)) => assert_eq!(a, b, "{code}"),
                    (Err(a), Err(b)) => {
                        let core = core(&b.to_string());
                        assert!(a.to_string().contains(&core), "{code}: ours {a}, Lua's {core}");
                    }
                    (a, b) => panic!("{code}: ours {a:?}, Lua's {b:?}"),
                }
            }
        }
    }

    fn show(v: &Value) -> String {
        match v {
            Value::String(s) => format!("{:?}", s.to_string_lossy()),
            Value::Nil => "nil".into(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            v => v.type_name().to_string(),
        }
    }

    /// The message of Lua's error, without its position (and inside a "bad argument" its reason).
    fn core(e: &str) -> String {
        let first = e.lines().next().unwrap_or("");
        let msg = first.split_once("case:1: ").map_or(first, |x| x.1);
        match msg.strip_prefix("bad argument").and_then(|m| m.split_once(" (")) {
            Some((_, why)) => why.trim_end_matches(')').to_string(),
            None => msg.to_string(),
        }
    }

    /// A backtracking pattern stops at the deadline instead of running for seconds.
    #[test]
    fn deadline_stops_backtracking() {
        let lua = Lua::new();
        let st = std::rc::Rc::new(std::cell::RefCell::new(super::super::vm::ModState::default()));
        st.borrow_mut().deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
        lua.set_app_data(st);
        super::install(&lua, &lua.globals().get("string").unwrap()).unwrap();
        let t0 = std::time::Instant::now();
        for code in [
            "return string.find(string.rep('a', 40000), '^.-.-b')",
            "return string.find(string.rep('a', 40000), '.-.-.-b')",
        ] {
            let r = lua.load(code).eval::<mlua::MultiValue>();
            assert!(r.unwrap_err().to_string().contains("time budget"), "{code}");
        }
        assert!(t0.elapsed() < std::time::Duration::from_secs(1), "took {:?}", t0.elapsed());
    }

    /// A huge set is charged by its length, so it can't scan for long between deadline checks.
    #[test]
    fn deadline_counts_large_sets() {
        let lua = Lua::new();
        let st = std::rc::Rc::new(std::cell::RefCell::new(super::super::vm::ModState::default()));
        st.borrow_mut().deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
        lua.set_app_data(st);
        super::install(&lua, &lua.globals().get("string").unwrap()).unwrap();
        let t0 = std::time::Instant::now();
        let r = lua
            .load("return string.find(string.rep('b', 1000000), '[' .. string.rep('a', 1000000) .. ']')")
            .eval::<mlua::MultiValue>();
        assert!(r.unwrap_err().to_string().contains("time budget"));
        assert!(t0.elapsed() < std::time::Duration::from_secs(1), "took {:?}", t0.elapsed());
    }

    /// A back-reference is charged by its length: long compares can't run for long between deadline checks.
    #[test]
    fn deadline_counts_back_references() {
        let lua = Lua::new();
        let st = std::rc::Rc::new(std::cell::RefCell::new(super::super::vm::ModState::default()));
        lua.set_app_data(st.clone());
        super::install(&lua, &lua.globals().get("string").unwrap()).unwrap();
        lua.load("s = string.rep('a', 16 << 20)").exec().unwrap();
        st.borrow_mut().deadline = Some(std::time::Instant::now() + std::time::Duration::from_millis(100));
        let t0 = std::time::Instant::now();
        let r = lua.load("return string.find(s, '^(.*)%1x')").eval::<mlua::MultiValue>();
        assert!(r.unwrap_err().to_string().contains("time budget"));
        assert!(t0.elapsed() < std::time::Duration::from_millis(500), "took {:?}", t0.elapsed());
    }

    /// gsub's output is held to the mod's memory limit, even within one replacement.
    #[test]
    fn gsub_output_is_bounded() {
        let lua = Lua::new();
        super::install(&lua, &lua.globals().get("string").unwrap()).unwrap();
        for code in [
            "return string.gsub(string.rep('a', 1000000), 'a', string.rep('x', 1048576))",
            "return string.gsub(string.rep('a', 1000000), '.+', string.rep('%0', 100))",
        ] {
            let r = lua.load(code).eval::<mlua::MultiValue>();
            assert!(r.unwrap_err().to_string().contains("not enough memory"), "{code}");
        }
    }
}
