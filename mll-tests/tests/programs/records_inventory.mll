-- An inventory: records with update syntax, grouping and totals per
-- category, restocking rules, price statistics at Number, and reports
-- built from sorted views.
-- Leans on: record construction/update/field selectors, derived Show of
-- records (with negative Number fields), sortBy with multi-key orders,
-- groupBy/partition/find from Data.List, Maybe chains, Number formatting.
import Data.List (sortBy, groupBy, partition, find)
import Data.Maybe (fromMaybe, mapMaybe)

data Category = Tools | Food | Toys
    deriving (Show, Eq, Ord)

data Item = Item
    { name :: String
    , category :: Category
    , qty :: Int
    , price :: Number
    , discount :: Number
    }
    deriving (Show, Eq)

inventory :: [Item]
inventory =
    [ Item { name = "hammer", category = Tools, qty = 12, price = 9.5, discount = 0 }
    , Item { name = "wrench", category = Tools, qty = 0, price = 12.25, discount = -0.5 }
    , Item { name = "apple", category = Food, qty = 200, price = 0.3, discount = 0.1 }
    , Item { name = "bread", category = Food, qty = 3, price = 2.15, discount = 0 }
    , Item { name = "kite", category = Toys, qty = 5, price = 15, discount = 0.25 }
    , Item { name = "yoyo", category = Toys, qty = 40, price = 1.75, discount = 0 }
    , Item { name = "saw", category = Tools, qty = 2, price = 22, discount = 0.05 }
    ]

effectivePrice :: Item -> Number
effectivePrice it = price it * (1 - discount it)

stockValue :: Item -> Number
stockValue it = effectivePrice it * fromInteger (toInteger (qty it))

byCategoryThenName :: Item -> Item -> Ordering
byCategoryThenName a b = case compare (category a) (category b) of
    EQ -> compare (name a) (name b)
    other -> other

byValueDesc :: Item -> Item -> Ordering
byValueDesc a b = compare (stockValue b) (stockValue a)

grouped :: [Item] -> [(Category, [Item])]
grouped items =
    [ (category (head g), g)
    | g <- groupBy (\a b -> category a == category b) (sortBy byCategoryThenName items) ]

restock :: Int -> Item -> Item
restock minimum' it
    | qty it < minimum' = it { qty = minimum', discount = 0 }
    | otherwise = it

applySale :: Category -> Number -> [Item] -> [Item]
applySale cat d = map (\it -> if category it == cat then it { discount = d } else it)

lookupItem :: String -> [Item] -> Maybe Item
lookupItem n = find (\it -> name it == n)

priceOf :: String -> [Item] -> Maybe Number
priceOf n items = fmap effectivePrice (lookupItem n items)

showMoney :: Number -> String
showMoney x = show x

report :: [Item] -> IO ()
report items = do
    mapM_ reportGroup (grouped items)
    putStrLn ("total value: " <> showMoney (sum (map stockValue items)))

joinWith :: String -> [String] -> String
joinWith _ [] = ""
joinWith _ [s] = s
joinWith sep (s : ss) = s <> sep <> joinWith sep ss

reportGroup :: (Category, [Item]) -> IO ()
reportGroup (cat, group) = do
    putStrLn (show cat <> ": " <> joinWith ", " (map name group))
    putStrLn ("  units " <> show (sum (map qty group)) <> ", value " <> showMoney (sum (map stockValue group)))

main :: IO ()
main = do
    print (head inventory)
    print (inventory !! 1)
    report inventory
    let (outOfStock, inStock) = partition (\it -> qty it == 0) inventory
    putStrLn ("out of stock: " <> show (map name outOfStock) <> ", in stock: " <> show (length inStock))
    putStrLn "by value:"
    mapM_ (\it -> putStrLn ("  " <> name it <> " " <> showMoney (stockValue it))) (sortBy byValueDesc inventory)
    let restocked = map (restock 10) inventory
    report restocked
    print (map (\it -> (name it, qty it)) restocked)
    let sale = applySale Food 0.5 restocked
    print (mapMaybe (\n -> priceOf n sale) ["apple", "bread", "hammer", "ghost"])
    print (fmap name (lookupItem "kite" sale), fmap name (lookupItem "ghost" sale))
    print (fromMaybe 0 (priceOf "ghost" sale), fromMaybe 0 (priceOf "wrench" sale))
    let renamed = (head sale) { name = "sledgehammer", price = 30 }
    print renamed
    print (renamed == head sale, renamed { name = "hammer", price = 9.5 } == head sale)
    print (maximum (map price sale), minimum (map effectivePrice sale))
    print (map (\it -> discount it) inventory)
