import LMath

newtype Rad = Rad Number
newtype Deg = Deg Number

rad :: Number -> Rad
rad = Rad

deg :: Number -> Deg
deg = Deg

main :: IO ()
main = do
    putStrLn $ "PI: " <> show pi
    putStrLn $ "Sin: " <> show (sin pi)
    let fr = frexp 0.75
    putStrLn $ "frexp 0.75 = " <> show fr
    let m = modf 3.75
    putStrLn $ "modf 3.75 = " <> show m
    print 17.23
    print (17, 23)
