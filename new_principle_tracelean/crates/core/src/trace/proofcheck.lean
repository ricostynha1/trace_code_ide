section TraceLeanCheck
open Lean Meta Elab Command Term

partial def TraceLeanCheck.reaches (env : Environment) (target : Name) : List Name → NameSet → Bool
  | [], _ => false
  | c :: rest, seen =>
    if seen.contains c then TraceLeanCheck.reaches env target rest seen
    else if c == target then true
    else
      let seen := seen.insert c
      match env.find? c with
      | some (.defnInfo d) => TraceLeanCheck.reaches env target (d.value.getUsedConstants.toList ++ rest) seen
      | some (.opaqueInfo d) => TraceLeanCheck.reaches env target (d.value.getUsedConstants.toList ++ rest) seen
      | _ => TraceLeanCheck.reaches env target rest seen

def TraceLeanCheck.leansOn (env : Environment) (target : Name) (e : Expr) : Option Name :=
  e.getUsedConstants.toList.find? fun c => c != target && TraceLeanCheck.reaches env target [c] {}

def TraceLeanCheck.general (f : Name) (e : Expr) : Bool :=
  (e.find? fun s =>
    s.isApp &&
      ((s.getAppFn.isConstOf f && s.getAppArgs.any (·.hasLooseBVars)) ||
       (s.getAppArgs.any (·.isConstOf f) && s.hasLooseBVars))).isSome

def TraceLeanCheck.resolve (n : Name) : CoreM Name := do
  let env ← getEnv
  if env.contains n then return n
  let found := env.constants.fold (init := none) fun acc c _ =>
    acc.orElse fun _ => if privateToUserName? c == some n then some c else none
  return found.getD n

def TraceLeanCheck.assumes (a : Name) : CoreM Bool := do
  match ← findDocString? (← getEnv) a with
  | some doc => return (doc.splitOn "@assumes").length > 1
  | none => return false

def TraceLeanCheck.axiomsOf (t : Name) : CoreM String := do
  let axioms ← Lean.collectAxioms t
  let mut words := #[]
  for a in axioms.toList do
    words := words.push s!"{a}:{if ← TraceLeanCheck.assumes a then "assumed" else "-"}"
  return if words.isEmpty then "-" else ",".intercalate words.toList

def TraceLeanCheck.closes (goal : Expr) : TermElabM Bool := do
  let saved ← Core.getMessageLog
  let m ← mkFreshExprMVar goal
  let attempt : TermElabM Bool := withCurrHeartbeats <| withTheReader Core.Context (fun c => { c with maxHeartbeats := 50000 * 1000 }) do
    let tac ← `(tactic| (intro _; intros; first | trivial | rfl | (simp_all; done) | decide))
    withoutErrToSorry (Lean.Elab.Term.runTactic m.mvarId! tac)
    let v ← instantiateMVars m
    pure (!v.hasSorry && !v.hasMVar && !(← Core.getMessageLog).hasErrors)
  let closed ← tryCatchRuntimeEx (try attempt catch _ => pure false) (fun _ => pure false)
  Core.setMessageLog saved
  pure closed

def TraceLeanCheck.abstractIn (f : Name) (g : Expr) (e : Expr) : Expr :=
  e.replace fun s => if s.isConstOf f then some g else none

def TraceLeanCheck.proofIgnores (f : Name) (ftype : Expr) (value goal : Expr) : MetaM Bool := do
  let attempt : MetaM Bool := do
    let lam ← withLocalDeclD `g ftype fun g => mkLambdaFVars #[g] (TraceLeanCheck.abstractIn f g value)
    if lam.getUsedConstants.contains f then return false
    let decl := Declaration.thmDecl
      { name := `_traceleanIgnores, levelParams := [], type := goal, value := lam }
    match (← getEnv).addDecl (← getOptions) decl with
    | .ok _ => return true
    | .error _ => return false
  tryCatchRuntimeEx (try attempt catch _ => pure false) (fun _ => pure false)

def TraceLeanCheck.isType (info : ConstantInfo) : MetaM Bool := do
  match info with
  | .inductInfo _ | .ctorInfo _ | .recInfo _ => return true
  | _ => forallTelescopeReducing info.type fun _ body => return body.isSort

def TraceLeanCheck.facts (shown f : Name) (finfo : ConstantInfo) (t : Name) : TermElabM Unit := do
  let env ← getEnv
  let some tinfo := env.find? (← TraceLeanCheck.resolve t) | IO.println s!"tracelean-facts {t} {shown} found=false"
  let isType ← TraceLeanCheck.isType finfo
  let ty ← instantiateMVars tinfo.type
  let mentions := ty.getUsedConstants.contains f
  let general := TraceLeanCheck.general f ty
  let leak := (TraceLeanCheck.leansOn env f ty).map toString |>.getD "-"
  let everyG ← withLocalDeclD `g finfo.type fun g => mkForallFVars #[g] (TraceLeanCheck.abstractIn f g ty)
  let decided := isType || !mentions || !general || leak != "-"
  let ignores ← if decided then pure false else match tinfo.value? with
    | some value => TraceLeanCheck.proofIgnores f finfo.type value everyG
    | none => pure false
  let closes ← if decided || ignores then pure false else TraceLeanCheck.closes everyG
  let axioms ← TraceLeanCheck.axiomsOf (← TraceLeanCheck.resolve t)
  IO.println s!"tracelean-facts {t} {shown} found=true type={isType} mentions={mentions} general={general} leak={leak} ignores={ignores} closes={closes} axioms={axioms}"

elab "#tracelean_facts " fid:ident " [" ts:ident,* "]" : command => liftTermElabM do
  let shown := fid.getId
  let f ← TraceLeanCheck.resolve shown
  let some finfo := (← getEnv).find? f | IO.println s!"tracelean-model {shown} missing"
  for t in ts.getElems do
    TraceLeanCheck.facts shown f finfo t.getId

elab "#tracelean_obligation " fid:ident " [" ts:ident,* "] [" ps:ident,* "]" : command => liftTermElabM do
  let env ← getEnv
  let shown := fid.getId
  let f ← TraceLeanCheck.resolve shown
  let some finfo := env.find? f | IO.println s!"tracelean-model {shown} missing"
  let mut specs := #[]
  for t in ts.getElems do
    if let some tinfo := env.find? (← TraceLeanCheck.resolve t.getId) then
      let ty ← instantiateMVars tinfo.type
      specs := specs.push (← withLocalDeclD `g finfo.type fun g => mkLambdaFVars #[g] (TraceLeanCheck.abstractIn f g ty))
  let obligation ← withLocalDeclD `g finfo.type fun g => do
    let concl ← mkEq g (mkConst f (finfo.levelParams.map mkLevelParam))
    let body ← specs.foldrM (fun s acc => mkArrow (s.beta #[g]) acc) concl
    mkForallFVars #[g] body
  IO.println s!"tracelean-obligation {shown}"
  IO.println s!"{← ppExpr obligation}"
  IO.println "tracelean-end"
  for p in ps.getElems do
    match env.find? (← TraceLeanCheck.resolve p.getId) with
    | none => IO.println s!"tracelean-pinfacts {p.getId} {shown} found=false"
    | some pinfo =>
      let statement ← tryCatchRuntimeEx (try isDefEq pinfo.type obligation catch _ => pure false) (fun _ => pure false)
      let axioms ← TraceLeanCheck.axiomsOf (← TraceLeanCheck.resolve p.getId)
      IO.println s!"tracelean-pinfacts {p.getId} {shown} found=true statement={statement} axioms={axioms}"

end TraceLeanCheck
