{-# LANGUAGE MonoLocalBinds #-}
{-# LANGUAGE ScopedTypeVariables #-}
{-# LANGUAGE TypeApplications #-}
{-# OPTIONS_GHC -Wno-deprecations #-}

-- | Tests for the functionality implemented in Concordium.Types.Updates.
module Types.UpdatesSpec where

import qualified Data.Aeson as AE
import qualified Data.Map as Map
import Data.Serialize hiding (label)
import qualified Data.Set as Set
import Test.Hspec
import Test.QuickCheck as QC

import Concordium.Crypto.DummyData (genSigSchemeKeyPair)
import qualified Concordium.Crypto.SignatureScheme as Sig
import Concordium.Types.Parameters
import Concordium.Types.ProtocolVersion
import Concordium.Types.Updates

import Generators

checkSerialization :: (Eq a, Show a) => Get a -> Putter a -> a -> Property
checkSerialization g p v = case runGet g (runPut $ p v) of
    Left err -> counterexample err False
    Right v' -> v' === v

checkUpdatePayloadSerialization :: SProtocolVersion pv -> UpdatePayload -> Property
checkUpdatePayloadSerialization spv = checkSerialization (getUpdatePayload spv) putUpdatePayload

checkUpdateInstructionSerialization :: SProtocolVersion pv -> UpdateInstruction -> Property
checkUpdateInstructionSerialization spv = checkSerialization (getUpdateInstruction spv) putUpdateInstruction

-- | Test that if we serialize then deserialize an 'UpdatePayload',
--  we get back the value we started with.
testSerializeUpdatePayload :: (IsProtocolVersion pv) => SProtocolVersion pv -> Property
testSerializeUpdatePayload spv =
    forAll (resize 50 $ genUpdatePayload $ sChainParametersVersionFor spv) $ checkUpdatePayloadSerialization $ spv

-- | Test that if we JSON-encode and decode an 'UpdatePayload',
--  we get back the value we started with.
testJSONUpdatePayload :: (IsChainParametersVersion cpv) => SChainParametersVersion cpv -> Property
testJSONUpdatePayload scpv = forAll (resize 50 $ genUpdatePayload scpv) chk
  where
    chk up = case AE.eitherDecode (AE.encode up) of
        Left err -> counterexample err False
        Right up' -> up === up'

-- | Function type for generating a set of keys to sign an update instruction with.
type SignKeyGen =
    -- available keys
    [Sig.KeyPair] ->
    -- a set of key indices authorized
    Set.Set UpdateKeyIndex ->
    -- The threshold
    Int ->
    -- The keys then used to sign the update
    Gen (Map.Map UpdateKeyIndex Sig.KeyPair)

-- | Generate an update instruction signed using the keys generated by the parameter.
--  The second argument indicates whether the signature should be valid.
testUpdateInstruction :: forall pv. (IsProtocolVersion pv) => SProtocolVersion pv -> SignKeyGen -> Bool -> Property
testUpdateInstruction spv keyGen isValid =
    forAll (withIsAuthorizationsVersionForPV (protocolVersion @pv) $ genKeyCollection @(AuthorizationsVersionForPV pv) 3) $ \(kc, rootK, level1K, level2K) ->
        forAll (genRawUpdateInstruction scpv) $ \rui -> do
            let p = ruiPayload rui
            keysToSign <- case p of
                RootUpdatePayload{} -> f p kc rootK
                Level1UpdatePayload{} -> f p kc level1K
                _ -> f p kc level2K
            let ui = makeUpdateInstruction rui keysToSign
            return $
                label "Signature check" (counterexample (show ui) $ isValid == checkAuthorizedUpdate kc ui)
                    .&&. label "Serialization check" (checkUpdateInstructionSerialization spv ui)
  where
    scpv = sChainParametersVersionFor spv
    f :: UpdatePayload -> UpdateKeysCollection cpv -> [Sig.KeyPair] -> Gen (Map.Map UpdateKeyIndex Sig.KeyPair)
    f pld ukc availableKeys = do
        let (keyIndices, thr) = extractKeysIndices pld ukc
        keyGen availableKeys keyIndices (fromIntegral thr)

-- | Make a collection of keys that should be sufficient to sign.
makeKeysGood :: SignKeyGen
makeKeysGood keys authIxs threshold = do
    nGoodSigs <- choose (threshold, Set.size authIxs)
    goodKeyIxs <- take nGoodSigs <$> shuffle (Set.toList authIxs)
    return $ Map.fromList [(k, keys !! fromIntegral k) | k <- goodKeyIxs]

-- | Make a collection of keys that are authorized but do not meet
--  the threshold for signing.
makeKeysFewGood :: SignKeyGen
makeKeysFewGood keys authIxs threshold = do
    nGoodSigs <- choose (1, threshold - 1)
    goodKeyIxs <- take nGoodSigs <$> shuffle (Set.toList authIxs)
    return $ Map.fromList [(k, keys !! fromIntegral k) | k <- goodKeyIxs]

-- | Make a collection of keys, none of which are authorized to sign.
makeKeysOther :: SignKeyGen
makeKeysOther keys authIxs _ = do
    if length keys == Set.size authIxs
        then do
            -- in this case, which can only happen when doing a level1 or root update
            -- the generated key will be an invalid one instead of a non-authorized one
            -- because there are no not-authorized keys in that case.
            -- Essentially it will just fail (as it should do).
            idx <- (length keys +) <$> choose (0, length keys - 1)
            Map.singleton (fromIntegral idx) <$> genSigSchemeKeyPair
        else do
            let otherKeys = [(i, k) | (i, k) <- [0 ..] `zip` keys, i `Set.notMember` authIxs]
            nKeys <- choose (1, length otherKeys)
            Map.fromList . take nKeys <$> shuffle otherKeys

-- | Make a key that is different to one in the keys.
makeKeyInvalid :: SignKeyGen
makeKeyInvalid keys _ _ = do
    idx <- choose (0, length keys - 1)
    let genKey = do
            k <- genSigSchemeKeyPair
            if k /= keys !! idx then return k else genKey
    Map.singleton (fromIntegral idx) <$> genKey

-- | Make a key that has an index that is out of bounds.
makeKeyBadIndex :: SignKeyGen
makeKeyBadIndex keys _ _ = do
    idx <- choose (fromIntegral (length keys), maxBound)
    Map.singleton idx <$> genSigSchemeKeyPair

-- | Combine two key generators, preferring the left one where indices overlap.
combineKeys :: SignKeyGen -> SignKeyGen -> SignKeyGen
combineKeys kg1 kg2 keys authIxs threshold = do
    k1 <- kg1 keys authIxs threshold
    k2 <- kg2 keys authIxs threshold
    return $ Map.union k1 k2

tests :: Spec
tests = parallel $ do
    specify "UpdatePayload JSON in CP0" $ withMaxSuccess 1000 $ testJSONUpdatePayload SChainParametersV0
    specify "UpdatePayload JSON in CP1" $ withMaxSuccess 1000 $ testJSONUpdatePayload SChainParametersV1
    specify "UpdatePayload JSON in CP2" $ withMaxSuccess 1000 $ testJSONUpdatePayload SChainParametersV2
    versionedTests SP1
    versionedTests SP2
    versionedTests SP3
    versionedTests SP4
    versionedTests SP5
    versionedTests SP6
    versionedTests SP7
  where
    versionedTests spv = describe (show $ demoteProtocolVersion spv) $ do
        specify "UpdatePayload serialization" $ withMaxSuccess 1000 $ testSerializeUpdatePayload spv
        specify "Valid update instructions" $ withMaxSuccess 1000 (testUpdateInstruction spv makeKeysGood True)
        specify "Valid update instructions, extraneous signatures" $ withMaxSuccess 1000 (testUpdateInstruction spv (combineKeys makeKeysOther makeKeysGood) False)
        specify "Update instructions, too few good" $ withMaxSuccess 1000 (testUpdateInstruction spv makeKeysFewGood False)
        specify "Update instructions, too few good, extraneous signatures" $ withMaxSuccess 1000 (testUpdateInstruction spv (combineKeys makeKeysOther makeKeysFewGood) False)
        specify "Update instructions, enough good, one bad" $ withMaxSuccess 1000 (testUpdateInstruction spv (combineKeys makeKeyInvalid makeKeysGood) False)
        specify "Update instructions, enough good, one bad (bad index)" $ withMaxSuccess 1000 (testUpdateInstruction spv (combineKeys makeKeyBadIndex makeKeysGood) False)
