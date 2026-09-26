/-! Strings: UTF-8 positions, slicing, transformation, and formatting. -/

def samples : List String :=
  ["", "a", "hello world", "héllo wörld", "日本語テキスト", "emoji 👩‍💻 and 🎉", "tab\there", "line\nbreak",
   "  padded  ", "Mixed Case ÄÖÜ", "quote\"s and \\backslash"]

def describe (s : String) : String :=
  s!"{repr s} len={s.length} bytes={s.utf8ByteSize} upper={s.toUpper} lower={s.toLower} " ++
  s!"rev={String.ofList s.toList.reverse} trim={repr s.trimAscii.toString} words={s.splitOn " "} " ++
  s!"front={repr s.front} back={repr s.back} drop2={s.drop 2} take3={s.take 3} " ++
  s!"cap={s.capitalize} isEmpty={s.isEmpty} hash={hash s}"

def positions (s : String) : List (Nat × Char) := Id.run do
  let mut out := []
  let mut p : String.Pos.Raw := 0
  while p < s.rawEndPos do
    out := (p.byteIdx, p.get s) :: out
    p := p.next s
  return out.reverse

def main : IO Unit := do
  for s in samples do IO.println (describe s)
  IO.println (positions "aé€𝄞z")
  IO.println ("a,b,,c".splitOn ",")
  IO.println (",".intercalate ["x", "y", "z"])
  IO.println ("abc" ++ "def" |>.push 'g' |>.append "h")
  IO.println ("hello".replace "l" "L")
  IO.println ("banana".toList.findIdx? (· == 'n'))
  IO.println (("abc".toList.map Char.toNat), 'x'.toNat, Char.ofNat 955)
  IO.println (decide ("abc" < "abd"), decide ("b" < "abc"), decide ("" < "a"), repr (compare "x" "x"))
  IO.println ("key=value".splitOn "=" |>.map String.length)
  IO.println (String.join ["α", "β", "γ"]).length
  IO.println ((List.range 20).foldl (fun acc i => acc ++ toString i) "")
  IO.println s!"{"Lean".startsWith "Le"} {"Lean".endsWith "an"} {"Lean".contains 'e'} {"Lean".any Char.isUpper}"
  IO.println (repr "\u0000\u0001\u007f€")
  IO.println ("  x y  ".trimAsciiStart.toString ++ "|" ++ "  x y  ".trimAsciiEnd.toString ++ "|")
  IO.println ((⟨1⟩ : String.Pos.Raw).get "abc", (⟨10⟩ : String.Pos.Raw).get "abc", (⟨2⟩ : String.Pos.Raw).get "héllo")
  IO.println ("Hello".map Char.toUpper, "Hello".foldl (fun n c => n + c.toNat) 0)
