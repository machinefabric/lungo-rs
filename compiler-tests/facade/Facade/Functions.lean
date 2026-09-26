namespace Facade

def applyTwice (f : Nat → Nat) (x : Nat) : Nat := f (f x)

def mapAll (f : Int → Int) (xs : List Int) : List Int := xs.map f

def makeAdder (n : Nat) : Nat → Nat := fun x => x + n

def foldWith (f : Nat → Nat → Nat) (init : Nat) (xs : Array Nat) : Nat := xs.foldl f init

def identity {α : Type} (x : α) : α := x

def pairUp {α β : Type} (a : α) (b : β) : α × β := (a, b)

def lengths {α : Type} (xs : List (List α)) : List Nat := xs.map List.length

end Facade
