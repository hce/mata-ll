-- Sir Humphrey's agenda: DataKinds version
-- Uses promoted constructors ('Empty, 'NonEmpty) as type-level state tags

-- Type-level tags to mark the state of the document
data AgendaState = Empty | NonEmpty

data TodaysActivity a state where
    -- An empty agenda has zero items
    AgendaVacua :: TodaysActivity a 'Empty

    -- An agendum has exactly one item
    Agendum     :: a -> TodaysActivity a 'NonEmpty

    -- An agenda has a non-empty list of items
    Agenda      :: a -> TodaysActivity a 'NonEmpty -> TodaysActivity a 'NonEmpty

describe :: TodaysActivity a s -> String
describe AgendaVacua       = "Nothing to discuss. The ideal meeting."
describe (Agendum _)       = "It's an agendum, Minister."
describe (Agenda _ _)      = "The agenda, Minister."

main :: IO ()
main = do
  putStrLn (describe AgendaVacua)
  putStrLn (describe (Agendum "Approve budget"))
  putStrLn (describe (Agenda "Approve budget" (Agendum "Blame predecessor")))
  putStrLn (describe (Agenda "Approve budget" (Agenda "Blame predecessor" (Agendum "Reorganise the Civil Service"))))
