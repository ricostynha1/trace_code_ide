-- Formal specification for REQ-03: Search Functionality
-- Datatypes ARE the spec. Code must produce exactly these structures.

/-- A single search hit. Every field is specified here — implementation must match. -/
structure SearchResult where
  file : String
  line : Nat
  snippet : String
  relevance : Float
  file_nonempty : file.length > 0
  line_positive : line > 0
  relevance_bounded : 0.0 ≤ relevance ∧ relevance ≤ 1.0

/-- Search options — defines what the user can configure. -/
structure SearchOptions where
  caseSensitive : Bool := false
  maxResults : Nat := 100
  filePattern : Option String := none

/-- All ways a search can fail. -/
inductive SearchError where
  | emptyQuery
  | invalidRegex (pattern : String)
  | timeout (elapsedMs : Nat)

/-- The search function signature — this IS the contract.
    Implementation must accept these args and return this type. -/
def SearchFn := String → List (String × String) → SearchOptions → Except SearchError (List SearchResult)

-- Properties

/-- Empty query always returns error, never results. -/
theorem empty_query_fails :
  ∀ (files : List (String × String)) (opts : SearchOptions) (f : SearchFn),
    f "" files opts = Except.error SearchError.emptyQuery := by
  sorry -- impl must satisfy

/-- Results are sorted descending by relevance. -/
theorem results_sorted :
  ∀ (results : List SearchResult),
    results.length > 1 →
    ∀ (i : Nat), i + 1 < results.length →
      (results.get ⟨i, by omega⟩).relevance ≥(results.get ⟨i+1, by omega⟩).relev
ance := by
  sorry -- impl must satisfy
-- Formal specification for REQ-03: Search Functionality
-- Datatypes ARE the spec. Code must produce exactly these structures.

/-- A single search hit. Every field is specified here — implementation must match. -/
structure SearchResult where
  file : String
  line : Nat
  snippet : String
  relevance : Float
  file_nonempty : file.length > 0
  line_positive : line > 0
  relevance_bounded : 0.0 ≤ relevance ∧ relevance ≤ 1.0

/-- Search options — defines what the user can configure. -/
structure SearchOptions where
  caseSensitive : Bool := false
  maxResults : Nat := 100
  filePattern : Option String := none

/-- All ways a search can fail. -/
inductive SearchError where
  | emptyQuery
  | invalidRegex (pattern : String)
  | timeout (elapsedMs : Nat)

/-- The search function signature — this IS the contract.
    Implementation must accept these args and return this type. -/
def SearchFn := String → List (String × String) → SearchOptions → Except SearchError (List SearchResult)

-- Properties

/-- Empty query always returns error, never results. -/
theorem empty_query_fails :
  ∀ (files : List (String × String)) (opts : SearchOptions) (f : SearchFn),
    f "" files opts = Except.error SearchError.emptyQuery := by
  sorry -- impl must satisfy

/-- Results are sorted descending by relevance. -/
theorem results_sorted :
  ∀ (results : List SearchResult),
    results.length > 1 →
    ∀ (i : Nat), i + 1 < results.length →
      (results.get ⟨i, by omega⟩).relevance ≥(results.get ⟨i+1, by omega⟩).relev
ance := by
  sorry -- impl must satisfy
