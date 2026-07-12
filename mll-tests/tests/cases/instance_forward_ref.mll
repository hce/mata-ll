-- Regression: instances are globally visible, so a method body may use an
-- instance declared LATER in the module. Instance identities are registered
-- for the whole module before any method body is checked; previously each
-- instance was registered only after its own body, so `show b` here failed
-- with "No instance for 'Show B'".
data A = MkA B
data B = MkB

instance Show A where
    show (MkA b) = "A " <> show b

instance Show B where
    show MkB = "B"

main :: IO ()
main = assert (show (MkA MkB) == "A B") "forward instance reference"
