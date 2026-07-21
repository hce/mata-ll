-- Sir Humphrey's agenda: GADT version with polymorphic items
-- The type system prevents constructing nonsensical agendas
-- (e.g. nesting DutyFree inside an Agenda)

data Empty = MkEmpty
data One = MkOne
data Many = MkMany

data Agenda a item where
  DutyFree :: Agenda Empty item    -- phantom in item: no value, like all useful work in Whitehall
  Agendum  :: item -> Agenda One item
  Items    :: item -> Agenda One item -> Agenda Many item

describe :: Agenda a item -> String
describe DutyFree     = "Nothing to discuss. The ideal meeting."
describe (Agendum _)  = "It's an agendum, Minister."
describe (Items _ _)  = "The agenda, Minister."

-- Same data constructors, different types
data Technically = MkTechnically
data ByUnspokenRules = MkByUnspokenRules

data Correctness a where
  NotWrong :: Correctness Technically
  Wrong    :: Correctness ByUnspokenRules

assess :: Correctness a -> String
assess NotWrong = "You're not wrong, Bernard."
assess Wrong    = "You're still wrong, Bernard."

main :: IO ()
main = do
  putStrLn (describe DutyFree)
  putStrLn (describe (Agendum "Approve budget"))
  putStrLn (describe (Items "Approve budget" (Agendum "Blame predecessor")))
  putStrLn (describe (Agendum 42))
  putStrLn (describe (Items 1 (Agendum 2)))
  let technically = NotWrong :: Correctness Technically
  let practically = Wrong :: Correctness ByUnspokenRules
  putStrLn (assess technically)
  putStrLn (assess practically)
