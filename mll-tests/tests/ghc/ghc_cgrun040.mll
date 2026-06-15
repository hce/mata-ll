-- GHC cgrun040: Type classes with custom instances
-- Tests user-defined typeclass with multiple instances

class Describable a where
    describe :: a -> String

data Animal = Cat | Dog | Fish
    deriving (Show, Eq)

instance Describable Animal where
    describe Cat  = "a furry feline"
    describe Dog  = "a loyal canine"
    describe Fish = "a scaly swimmer"

data Priority = Low | Medium | High
    deriving (Show, Eq, Ord)

instance Describable Priority where
    describe Low    = "low priority"
    describe Medium = "medium priority"
    describe High   = "high priority"

main :: IO ()
main = do
    assert (describe Cat == "a furry feline") "describe Cat"
    assert (describe Dog == "a loyal canine") "describe Dog"
    assert (describe Fish == "a scaly swimmer") "describe Fish"
    assert (describe Low == "low priority") "describe Low"
    assert (describe High == "high priority") "describe High"
    putStrLn "ok"
