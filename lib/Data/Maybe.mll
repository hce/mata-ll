module Data.Maybe
    ( maybe, isJust, isNothing, fromJust, fromMaybe
    , listToMaybe, maybeToList, catMaybes, mapMaybe
    ) where

maybe :: b -> (a -> b) -> Maybe a -> b
maybe def _ Nothing = def
maybe _ f (Just x) = f x

isJust :: Maybe a -> Bool
isJust (Just _) = True
isJust Nothing = False

isNothing :: Maybe a -> Bool
isNothing Nothing = True
isNothing (Just _) = False

fromJust :: Maybe a -> a
fromJust (Just x) = x
fromJust Nothing = error "Data.Maybe.fromJust: Nothing"

fromMaybe :: a -> Maybe a -> a
fromMaybe def Nothing = def
fromMaybe _ (Just x) = x

listToMaybe :: [a] -> Maybe a
listToMaybe [] = Nothing
listToMaybe (x:_) = Just x

maybeToList :: Maybe a -> [a]
maybeToList Nothing = []
maybeToList (Just x) = [x]

catMaybes :: [Maybe a] -> [a]
catMaybes [] = []
catMaybes (Nothing:xs) = catMaybes xs
catMaybes (Just x:xs) = x : catMaybes xs

mapMaybe :: (a -> Maybe b) -> [a] -> [b]
mapMaybe _ [] = []
mapMaybe f (x:xs) = case f x of
    Nothing -> mapMaybe f xs
    Just y -> y : mapMaybe f xs
