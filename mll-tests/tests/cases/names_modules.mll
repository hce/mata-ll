import qualified NamesA as A
import qualified NamesA_b as B

data Boxed = NamesA_b_step Int | NamesA_Tok Int deriving (Show, Eq)

show_Int :: Int -> String
show_Int n = "Main:" <> show n

eq_Int :: Int -> Int -> Bool
eq_Int _ _ = True

head_ :: [Int] -> Int
head_ xs = if null xs then 0 else head xs + head_ (drop 1 xs)

main :: IO ()
main = do
    k <- pure (3 :: Int)
    print (A.b_step k, B.step k, NamesA_b_step k, NamesA_Tok k)
    print (A.Tok k, B.Tok2 k, A.Tok k == A.Tok 3, B.Tok2 k < B.Tok2 4)
    putStrLn (show_Int k <> " " <> A.show_Int k <> " " <> B.show_Int k <> " " <> A.show_Tok (A.Tok k))
    print (eq_Int k 9, B.eq_Int k 4, k == 9, k == 3)
    print (A.scale k 4, head_ [k, 4])
    putStrLn (A.render k)
