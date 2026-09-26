import Imports.Base

namespace Imports

/-- Depends on a definition from an imported local module. -/
def scaled (n : Nat) : Nat := n * Imports.Base.factor + Imports.Base.offset

def greeting (name : String) : String := s!"{Imports.Base.salutation}, {name}!"

end Imports
