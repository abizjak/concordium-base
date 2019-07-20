{-# LANGUAGE ScopedTypeVariables #-}
module Concordium.Crypto.ByteStringHelpers where

import           Text.Printf
import qualified Data.FixedByteString as FBS
import           Foreign.Ptr
import           Data.Word
import qualified Data.List as L
import           Control.Monad
import Data.Serialize
import qualified Data.ByteString.Base16 as BS16
import Data.Text.Encoding as Text

import Control.Monad.Fail(MonadFail)
import qualified Data.Aeson as AE
import qualified Data.Aeson.Types as AE
import qualified Data.Text as Text
import Prelude hiding (fail)

import Foreign.Marshal
import Data.ByteString(ByteString)
import Data.ByteString.Short (ShortByteString)
import qualified Data.ByteString.Short as BSS
import qualified Data.ByteString.Short.Internal as BSS
import qualified Data.ByteString.Unsafe as BSU
import qualified Data.ByteString as BS

wordToHex :: Word8 -> [Char]
wordToHex x = printf "%.2x" x

byteStringToHex :: ByteString -> String
byteStringToHex b= L.concatMap wordToHex ls
    where
        ls = BS.unpack b

{-# INLINE withByteStringPtr #-}
withByteStringPtr :: ShortByteString -> (Ptr Word8 -> IO a) -> IO a
withByteStringPtr bs f = BSU.unsafeUseAsCString (BSS.fromShort bs) (f . castPtr)

{-# INLINE withAllocatedShortByteString #-}
withAllocatedShortByteString :: Int -> (Ptr Word8 -> IO a) -> IO (a, ShortByteString)
withAllocatedShortByteString n f =
  allocaBytes n $ \ptr -> do
  r <- f ptr
  sbs <- BSS.createFromPtr ptr n
  return (r, sbs)

fbsHex :: FBS.FixedLength a => FBS.FixedByteString a -> String
fbsHex = byteStringToHex . FBS.toByteString

fbsPut :: FBS.FixedLength a => FBS.FixedByteString a -> Put
fbsPut = putByteString . FBS.toByteString

fbsGet :: forall a . FBS.FixedLength a => Get (FBS.FixedByteString a)
fbsGet = FBS.fromByteString <$> getByteString (FBS.fixedLength (undefined :: a))

-- |Wrapper used to automatically derive Show instances in base16 for types
-- simply wrapping bytestrings.
newtype ByteStringHex = ByteStringHex ShortByteString

instance Show ByteStringHex where
  show (ByteStringHex s) = byteStringToHex (BSS.fromShort s)

-- |Wrapper used to automatically derive Show instances in base16 for types
-- simply wrapping fixed byte stringns.
newtype FBSHex a = FBSHex (FBS.FixedByteString a)

instance FBS.FixedLength a => Show (FBSHex a) where
  show (FBSHex s) = fbsHex s

instance FBS.FixedLength a => Serialize (FBSHex a) where
  put (FBSHex s) = fbsPut s
  get = FBSHex <$> fbsGet

-- |Type whose only purpose is to enable derivation of serialization instances.
newtype Short65K = Short65K ShortByteString

instance Serialize Short65K where
  put (Short65K bs) =
    putWord16be (fromIntegral (BSS.length bs)) <>
    putShortByteString bs
  get = do
    l <- fromIntegral <$> getWord16be
    Short65K <$> getShortByteString l

instance Show Short65K where
  show (Short65K s) = byteStringToHex (BSS.fromShort s)

-- |JSON instances based on base16 encoding.
instance AE.ToJSON Short65K where
  toJSON v = AE.String (Text.pack (show v))

-- |JSON instances based on base16 encoding.
instance AE.FromJSON Short65K where
  parseJSON = AE.withText "Short65K" $ \t ->
    let (bs, rest) = BS16.decode (Text.encodeUtf8 t)
    in if BS.null rest then return (Short65K (BSS.toShort bs))
       else AE.typeMismatch "Not a valid Base16 encoding." (AE.String t)


-- |JSON instances based on base16 encoding.
instance AE.ToJSON ByteStringHex where
  toJSON v = AE.String (Text.pack (show v))

-- |JSON instances based on base16 encoding.
instance AE.FromJSON ByteStringHex where
  parseJSON = AE.withText "ByteStringHex" $ \t ->
    let (bs, rest) = BS16.decode (Text.encodeUtf8 t)
    in if BS.null rest then return (ByteStringHex (BSS.toShort bs))
       else AE.typeMismatch "Not a valid Base16 encoding." (AE.String t)

-- |Use the serialize instance of a type to deserialize 
deserializeBase16 :: (Serialize a, MonadFail m) => Text.Text -> m a
deserializeBase16 t =
        if BS.null rest then
            case decode bs of
                Left er -> fail er
                Right r -> return r
        else
            fail $ "Could not decode as base-16: " ++ show t
    where
        (bs, rest) = BS16.decode (Text.encodeUtf8 t)

-- |Use the serialize instance to convert from base 16 to value, but add
-- explicit length as 4 bytes big endian in front.
deserializeBase16WithLength4 :: (Serialize a, MonadFail m) => Text.Text -> m a
deserializeBase16WithLength4 t =
        if BS.null rest then
            case decode (runPut (putWord32be (fromIntegral (BS.length bs))) <> bs) of
                Left er -> fail er
                Right r -> return r
        else
            fail $ "Could not decode as base-16: " ++ show t
    where
        (bs, rest) = BS16.decode (Text.encodeUtf8 t)


serializeBase16 :: (Serialize a) => a -> Text.Text
serializeBase16 = Text.decodeUtf8 . BS16.encode . encode

-- |Serialize a type whose serialization puts an explicit length up front.
-- The length is 4 bytes and is cut off by this function.
serializeBase16WithLength4 :: (Serialize a) => a -> Text.Text
serializeBase16WithLength4 = Text.decodeUtf8 . BS16.encode . BS.drop 4 . encode
