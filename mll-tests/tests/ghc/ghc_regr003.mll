-- ghc_regr003: Typeclass method called on result of another typeclass method

class Describable a where
    describe :: a -> String

class Sizeable a where
    size :: a -> Integer

data Color = Red | Green | Blue
    deriving (Show, Eq)

data Widget = Button String | Label String | Container Integer
    deriving (Show, Eq)

instance Describable Color where
    describe Red   = "red"
    describe Green = "green"
    describe Blue  = "blue"

instance Sizeable Color where
    size _ = 1

instance Describable Widget where
    describe (Button s)    = "button:" <> s
    describe (Label s)     = "label:" <> s
    describe (Container n) = "container:" <> show n

instance Sizeable Widget where
    size (Button _)    = 1
    size (Label _)     = 1
    size (Container n) = n

-- Single-constraint functions that use typeclass methods on results of other methods
showDescribed :: Describable a => a -> String
showDescribed x = show (describe x)

doubleSized :: Sizeable a => a -> Integer
doubleSized x = size x * 2

main :: IO ()
main = do
    assert (describe Red == "red") "describe Red"
    assert (describe Green == "green") "describe Green"
    assert (describe (Button "ok") == "button:ok") "describe Button"
    assert (describe (Label "hi") == "label:hi") "describe Label"
    assert (describe (Container 5) == "container:5") "describe Container"

    assert (size Red == 1) "size color"
    assert (size (Button "x") == 1) "size button"
    assert (size (Container 7) == 7) "size container"

    -- Method on result of another method
    assert (doubleSized Red == 2) "doubleSize color"
    assert (doubleSized (Container 3) == 6) "doubleSize container"

    -- show on describe result (typeclass method on typeclass method result)
    -- In mll, show of String is identity (no quotes added)
    assert (showDescribed Red == "red") "showDescribed color"
    assert (showDescribed (Button "ok") == "button:ok") "showDescribed button"

    -- Arithmetic on size results
    assert (size Red + size (Button "x") == 2) "size sum"
    assert (size (Container 10) - size (Label "hi") == 9) "size diff"

    putStrLn "ok"
