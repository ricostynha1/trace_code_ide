-- Formal specification for REQ-02: File Upload
-- Types first. The implementation derives from these.

/-- Upload size limit — hardcoded in the spec, implementation must respect. -/
def maxUploadBytes : Nat := 100 * 1024 * 1024  -- 100MB

/-- An upload request. Every field's constraints are in the type. -/
structure UploadRequest where
  filename : String
  sizeBytes : Nat
  data : ByteArray
  filename_nonempty : filename.length > 0
  size_matches_data : sizeBytes = data.size

/-- Where the file ends up after successful upload. -/
structure UploadResult where
  savedPath : String
  finalSize : Nat

/-- All failure modes — implementation must handle each case. -/
inductive UploadError where
  | emptyFilename
  | tooLarge (actualSize : Nat) (limit : Nat)
  | ioError (message : String)
  | invalidExtension (ext : String)
  | diskFull

/-- The upload function contract. -/
def UploadFn := UploadRequest → String → Except UploadError UploadResult

-- Properties

theorem size_limit_enforced :
  ∀ (req : UploadRequest) (workspace : String) (f : UploadFn),
    req.sizeBytes > maxUploadBytes →
    ∃ e, f req workspace = Except.error e := by
  sorry

theorem saved_path_within_workspace :
  ∀ (req : UploadRequest) (workspace : String) (f : UploadFn) (result : UploadResult),
    f req workspace = Except.ok result →
    result.savedPath.startsWith workspace := by
  sorry
