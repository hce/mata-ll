-- Container abstractions: a higher-kinded Container class with a stack
-- and a banker's queue instance, a Shape class with a default method and
-- an existential wrapper, and Enum/Bounded enumeration.
-- Leans on: user classes over type constructors, class default methods,
-- existential constructors with a class context, deriving Enum and
-- Bounded with [minBound .. maxBound], Number formatting, newtypes.
class Container f where
    empty :: f a
    insert :: a -> f a -> f a
    remove :: f a -> Maybe (a, f a)
    contents :: f a -> [a]

newtype Stack a = Stack [a]

instance Container Stack where
    empty = Stack []
    insert x (Stack xs) = Stack (x : xs)
    remove (Stack []) = Nothing
    remove (Stack (x : xs)) = Just (x, Stack xs)
    contents (Stack xs) = xs

-- Front list for removal, back list for insertion, reversed on demand.
data Queue a = Queue [a] [a]

instance Container Queue where
    empty = Queue [] []
    insert x (Queue front back) = Queue front (x : back)
    remove (Queue [] []) = Nothing
    remove (Queue [] back) = remove (Queue (reverse back) [])
    remove (Queue (x : front) back) = Just (x, Queue front back)
    contents (Queue front back) = front ++ reverse back

fillFrom :: Container f => [a] -> f a
fillFrom = foldl (\c x -> insert x c) empty

drain :: Container f => f a -> [a]
drain c = case remove c of
    Nothing -> []
    Just (x, rest) -> x : drain rest

-- Interleave removals and insertions to exercise the queue's rotation.
churn :: Container f => f Int -> Int -> [Int]
churn c 0 = drain c
churn c n = case remove c of
    Nothing -> []
    Just (x, rest) -> x : churn (insert (x * 10) rest) (n - 1)

class Shape a where
    area :: a -> Number
    perimeter :: a -> Number
    name :: a -> String
    describe :: a -> String
    describe s = name s <> " with area " <> show (area s) <> " and perimeter " <> show (perimeter s)

data Rect = Rect Number Number
data Circle = Circle Number
data Square = Square Number

instance Shape Rect where
    area (Rect w h) = w * h
    perimeter (Rect w h) = 2 * (w + h)
    name _ = "rectangle"

instance Shape Circle where
    area (Circle r) = 3.25 * r * r
    perimeter (Circle r) = 2 * 3.25 * r
    name _ = "circle"
    describe c = "a round " <> name c <> " of area " <> show (area c)

instance Shape Square where
    area (Square s) = s * s
    perimeter (Square s) = 4 * s
    name _ = "square"

data AnyShape = forall s. Shape s => AnyShape s

describeAny :: AnyShape -> String
describeAny (AnyShape s) = describe s

totalArea :: [AnyShape] -> Number
totalArea shapes = sum [area s | AnyShape s <- shapes]

largest :: [AnyShape] -> String
largest [] = "nothing"
largest (AnyShape first : rest) = go (name first) (area first) rest
  where
    go best _ [] = best
    go best bestArea (AnyShape s : more)
        | area s > bestArea = go (name s) (area s) more
        | otherwise = go best bestArea more

data Weekday = Mon | Tue | Wed | Thu | Fri | Sat | Sun
    deriving (Show, Eq, Ord, Enum, Bounded)

isWeekend :: Weekday -> Bool
isWeekend d = d >= Sat

nextDay :: Weekday -> Weekday
nextDay d = if d == maxBound then minBound else succ d

main :: IO ()
main = do
    print (contents (fillFrom [1, 2, 3, 4] :: Stack Int))
    print (contents (fillFrom [1, 2, 3, 4] :: Queue Int))
    print (drain (fillFrom [1, 2, 3, 4] :: Stack Int))
    print (drain (fillFrom [1, 2, 3, 4] :: Queue Int))
    print (churn (fillFrom [1, 2, 3] :: Queue Int) 5)
    print (churn (fillFrom [1, 2, 3] :: Stack Int) 5)
    print (drain (empty :: Queue Int), drain (empty :: Stack Int))
    let shapes = [AnyShape (Rect 3 4), AnyShape (Circle 2), AnyShape (Square 2.5), AnyShape (Rect 0.5 0.25)]
    mapM_ (putStrLn . describeAny) shapes
    print (totalArea shapes)
    putStrLn (largest shapes)
    putStrLn (largest [])
    print [minBound .. maxBound :: Weekday]
    print (map isWeekend [minBound .. maxBound])
    print (map nextDay [Mon, Fri, Sun])
    print (fromEnum Thu, toEnum 5 :: Weekday, [Mon, Wed ..], succ Mon, pred Sun)
    print (maximum [Tue, Sun, Mon], minimum [Tue, Sun, Mon], compare Wed Tue)
