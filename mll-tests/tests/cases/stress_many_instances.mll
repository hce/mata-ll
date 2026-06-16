-- Stress test: many data types each with Show and Eq instances

class Describable a where
    describe :: a -> String

data T1 = T1A | T1B Integer
    deriving (Show, Eq)

instance Describable T1 where
    describe T1A = "T1A"
    describe (T1B n) = "T1B:" <> show n

data T2 = T2A | T2B Integer
    deriving (Show, Eq)

instance Describable T2 where
    describe T2A = "T2A"
    describe (T2B n) = "T2B:" <> show n

data T3 = T3A | T3B Integer
    deriving (Show, Eq)

instance Describable T3 where
    describe T3A = "T3A"
    describe (T3B n) = "T3B:" <> show n

data T4 = T4A | T4B Integer
    deriving (Show, Eq)

instance Describable T4 where
    describe T4A = "T4A"
    describe (T4B n) = "T4B:" <> show n

data T5 = T5A | T5B Integer
    deriving (Show, Eq)

instance Describable T5 where
    describe T5A = "T5A"
    describe (T5B n) = "T5B:" <> show n

data T6 = T6A | T6B Integer
    deriving (Show, Eq)

instance Describable T6 where
    describe T6A = "T6A"
    describe (T6B n) = "T6B:" <> show n

data T7 = T7A | T7B Integer
    deriving (Show, Eq)

instance Describable T7 where
    describe T7A = "T7A"
    describe (T7B n) = "T7B:" <> show n

data T8 = T8A | T8B Integer
    deriving (Show, Eq)

instance Describable T8 where
    describe T8A = "T8A"
    describe (T8B n) = "T8B:" <> show n

data T9 = T9A | T9B Integer
    deriving (Show, Eq)

instance Describable T9 where
    describe T9A = "T9A"
    describe (T9B n) = "T9B:" <> show n

data T10 = T10A | T10B Integer
    deriving (Show, Eq)

instance Describable T10 where
    describe T10A = "T10A"
    describe (T10B n) = "T10B:" <> show n

data T11 = T11A | T11B Integer
    deriving (Show, Eq)

instance Describable T11 where
    describe T11A = "T11A"
    describe (T11B n) = "T11B:" <> show n

data T12 = T12A | T12B Integer
    deriving (Show, Eq)

instance Describable T12 where
    describe T12A = "T12A"
    describe (T12B n) = "T12B:" <> show n

data T13 = T13A | T13B Integer
    deriving (Show, Eq)

instance Describable T13 where
    describe T13A = "T13A"
    describe (T13B n) = "T13B:" <> show n

data T14 = T14A | T14B Integer
    deriving (Show, Eq)

instance Describable T14 where
    describe T14A = "T14A"
    describe (T14B n) = "T14B:" <> show n

data T15 = T15A | T15B Integer
    deriving (Show, Eq)

instance Describable T15 where
    describe T15A = "T15A"
    describe (T15B n) = "T15B:" <> show n

data T16 = T16A | T16B Integer
    deriving (Show, Eq)

instance Describable T16 where
    describe T16A = "T16A"
    describe (T16B n) = "T16B:" <> show n

data T17 = T17A | T17B Integer
    deriving (Show, Eq)

instance Describable T17 where
    describe T17A = "T17A"
    describe (T17B n) = "T17B:" <> show n

data T18 = T18A | T18B Integer
    deriving (Show, Eq)

instance Describable T18 where
    describe T18A = "T18A"
    describe (T18B n) = "T18B:" <> show n

data T19 = T19A | T19B Integer
    deriving (Show, Eq)

instance Describable T19 where
    describe T19A = "T19A"
    describe (T19B n) = "T19B:" <> show n

data T20 = T20A | T20B Integer
    deriving (Show, Eq)

instance Describable T20 where
    describe T20A = "T20A"
    describe (T20B n) = "T20B:" <> show n

data T21 = T21A | T21B Integer
    deriving (Show, Eq)

instance Describable T21 where
    describe T21A = "T21A"
    describe (T21B n) = "T21B:" <> show n

data T22 = T22A | T22B Integer
    deriving (Show, Eq)

instance Describable T22 where
    describe T22A = "T22A"
    describe (T22B n) = "T22B:" <> show n

data T23 = T23A | T23B Integer
    deriving (Show, Eq)

instance Describable T23 where
    describe T23A = "T23A"
    describe (T23B n) = "T23B:" <> show n

data T24 = T24A | T24B Integer
    deriving (Show, Eq)

instance Describable T24 where
    describe T24A = "T24A"
    describe (T24B n) = "T24B:" <> show n

data T25 = T25A | T25B Integer
    deriving (Show, Eq)

instance Describable T25 where
    describe T25A = "T25A"
    describe (T25B n) = "T25B:" <> show n

data T26 = T26A | T26B Integer
    deriving (Show, Eq)

instance Describable T26 where
    describe T26A = "T26A"
    describe (T26B n) = "T26B:" <> show n

data T27 = T27A | T27B Integer
    deriving (Show, Eq)

instance Describable T27 where
    describe T27A = "T27A"
    describe (T27B n) = "T27B:" <> show n

data T28 = T28A | T28B Integer
    deriving (Show, Eq)

instance Describable T28 where
    describe T28A = "T28A"
    describe (T28B n) = "T28B:" <> show n

data T29 = T29A | T29B Integer
    deriving (Show, Eq)

instance Describable T29 where
    describe T29A = "T29A"
    describe (T29B n) = "T29B:" <> show n

data T30 = T30A | T30B Integer
    deriving (Show, Eq)

instance Describable T30 where
    describe T30A = "T30A"
    describe (T30B n) = "T30B:" <> show n

describeAndShow :: (Describable a, Show a) => a -> String
describeAndShow x = describe x <> " (" <> show x <> ")"

main :: IO ()
main = do
    assert (describe T1A == "T1A") "describe T1A"
    assert (T1B 42 == T1B 42) "eq T1B"
    assert (T1B 1 /= T1B 2) "neq T1B"
    assert (describe T5A == "T5A") "describe T5A"
    assert (T5B 42 == T5B 42) "eq T5B"
    assert (T5B 1 /= T5B 2) "neq T5B"
    assert (describe T10A == "T10A") "describe T10A"
    assert (T10B 42 == T10B 42) "eq T10B"
    assert (T10B 1 /= T10B 2) "neq T10B"
    assert (describe T15A == "T15A") "describe T15A"
    assert (T15B 42 == T15B 42) "eq T15B"
    assert (T15B 1 /= T15B 2) "neq T15B"
    assert (describe T20A == "T20A") "describe T20A"
    assert (T20B 42 == T20B 42) "eq T20B"
    assert (T20B 1 /= T20B 2) "neq T20B"
    assert (describe T25A == "T25A") "describe T25A"
    assert (T25B 42 == T25B 42) "eq T25B"
    assert (T25B 1 /= T25B 2) "neq T25B"
    assert (describe T30A == "T30A") "describe T30A"
    assert (T30B 42 == T30B 42) "eq T30B"
    assert (T30B 1 /= T30B 2) "neq T30B"
    assert (describeAndShow T1A == "T1A (T1A)") "describeAndShow"
    putStrLn "ok"
