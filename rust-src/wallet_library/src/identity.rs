use anyhow::{bail, ensure, Result};
use concordium_base::{
    common::*,
    id::{
        account_holder::generate_pio_v1, constants, constants::ArCurve,
        dodis_yampolskiy_prf as prf, pedersen_commitment::Value as PedersenValue,
        secret_sharing::Threshold, types::*,
    },
};
use key_derivation::{ConcordiumHdWallet, Net};
use serde::{Deserialize as SerdeDeserialize, Serialize as SerdeSerialize};
use serde_json::{json, to_string};
use std::{collections::BTreeMap, convert::TryInto};

type JsonString = String;

#[derive(SerdeSerialize, SerdeDeserialize)]
#[serde(rename_all = "camelCase")]
struct IdentityObjectRequestV1 {
    id_object_request: Versioned<PreIdentityObjectV1<constants::IpPairing, ArCurve>>,
}

#[derive(SerdeSerialize, SerdeDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdRequestCommon {
    ip_info:        IpInfo<constants::IpPairing>,
    global_context: GlobalContext<constants::ArCurve>,
    ars_infos:      BTreeMap<ArIdentity, ArInfo<constants::ArCurve>>,
    net:            Net,
    identity_index: u32,
    ar_threshold:   u8,
}

#[derive(SerdeSerialize, SerdeDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdRequestInput {
    common: IdRequestCommon,
    seed:   String,
}

#[derive(SerdeSerialize, SerdeDeserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdRequestInputWithKeys {
    common:              IdRequestCommon,
    prf_key:             prf::SecretKey<ArCurve>,
    id_cred_sec:         PedersenValue<ArCurve>,
    // This does not have serde serializers / deserializers.
    blinding_randomness: String,
}

pub fn create_id_request_with_keys_v1_aux(input: IdRequestInputWithKeys) -> Result<JsonString> {
    let prf_key: prf::SecretKey<ArCurve> = input.prf_key;
    let id_cred_sec: PedersenValue<ArCurve> = input.id_cred_sec;
    let id_cred: IdCredentials<ArCurve> = IdCredentials { id_cred_sec };
    let sig_retrieval_randomness: concordium_base::id::ps_sig::SigRetrievalRandomness<
        constants::IpPairing,
    > = base16_decode_string(&input.blinding_randomness)?;

    let num_of_ars = input.common.ars_infos.len();
    ensure!(
        input.common.ar_threshold > 0,
        "arThreshold must be at least 1."
    );
    ensure!(
        num_of_ars >= usize::from(input.common.ar_threshold),
        "Number of anonymity revokers in arsInfos should be at least arThreshold."
    );

    let threshold = Threshold(input.common.ar_threshold);
    let chi = CredentialHolderInfo::<ArCurve> { id_cred };
    let aci = AccCredentialInfo {
        cred_holder_info: chi,
        prf_key,
    };

    let context = IpContext::new(
        &input.common.ip_info,
        &input.common.ars_infos,
        &input.common.global_context,
    );
    let id_use_data = IdObjectUseData {
        aci,
        randomness: sig_retrieval_randomness,
    };
    let (pio, _) = {
        match generate_pio_v1(&context, threshold, &id_use_data) {
            Some(x) => x,
            None => bail!("Generating the pre-identity object failed."),
        }
    };

    let response = json!({ "idObjectRequest": Versioned::new(VERSION_0, pio) });
    Ok(to_string(&response)?)
}

/// Creates an identity object request where the supplied seed phrase is
/// used to derive the keys.
pub fn create_id_request_v1_aux(input: IdRequestInput) -> Result<JsonString> {
    let seed_decoded = hex::decode(&input.seed)?;
    let seed: [u8; 64] = match seed_decoded.try_into() {
        Ok(s) => s,
        Err(_) => bail!("The provided seed {} was not 64 bytes", input.seed),
    };

    let wallet: ConcordiumHdWallet = ConcordiumHdWallet { seed, net: input.common.net };

    let identity_provider_index = input.common.ip_info.ip_identity.0;
    let prf_key: prf::SecretKey<ArCurve> =
        wallet.get_prf_key(identity_provider_index, input.common.identity_index)?;
    let id_cred_sec: PedersenValue<ArCurve> = PedersenValue::new(
        wallet.get_id_cred_sec(identity_provider_index, input.common.identity_index)?,
    );
    let blinding_randomness: concordium_base::id::ps_sig::SigRetrievalRandomness<
        constants::IpPairing,
    > = wallet.get_blinding_randomness(identity_provider_index, input.common.identity_index)?;

    let input = IdRequestInputWithKeys {
        common: input.common,
        prf_key,
        id_cred_sec,
        blinding_randomness: base16_encode_string(&blinding_randomness),
    };

    create_id_request_with_keys_v1_aux(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    const TEST_SEED_1: &str = "efa5e27326f8fa0902e647b52449bf335b7b605adc387015ec903f41d95080eb71361cbc7fb78721dcd4f3926a337340aa1406df83332c44c1cdcfe100603860";

    fn read_test_data(ar_threshold: u8, identity_index: u32, net: Net) -> IdRequestCommon {
        let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        d.push("resources");
        let base_path = d
            .as_path()
            .as_os_str()
            .to_str()
            .expect("Should be able to get base path.");

        let contents = fs::read_to_string(format!("{}/{}", &base_path, "ip_info.json"))
            .expect("Should have been able to read the file");
        let ip_info_versioned: Versioned<IpInfo<constants::IpPairing>> =
            serde_json::from_str(contents.as_str()).unwrap();
        let ip_info = ip_info_versioned.value;

        let ar_info_contents = fs::read_to_string(format!("{}/{}", &base_path, "ars_infos.json"))
            .expect("Should have been able to read the file");
        let ar_info_versioned: Versioned<BTreeMap<ArIdentity, ArInfo<constants::ArCurve>>> =
            serde_json::from_str(&ar_info_contents).unwrap();
        let ars_infos = ar_info_versioned.value;

        let global_contents = fs::read_to_string(format!("{}/{}", &base_path, "global.json"))
            .expect("Should have been able to read the file");
        let global_versioned: Versioned<GlobalContext<constants::ArCurve>> =
            serde_json::from_str(&global_contents).unwrap();
        let global_context = global_versioned.value;

        IdRequestCommon {
            ip_info,
            ars_infos,
            global_context,
            ar_threshold,
            identity_index,
            net,
        }
    }

    #[test]
    pub fn create_id_request_with_seed_phrase() {
        let ar_threshold = 2;
        let test_data = read_test_data(ar_threshold.clone(), 0, Net::Testnet);

        let input: IdRequestInput = IdRequestInput {
            common: test_data,
            seed:   TEST_SEED_1.to_string(),
        };
        let request_string = create_id_request_v1_aux(input).unwrap();
        let request: IdentityObjectRequestV1 = serde_json::from_str(&request_string).unwrap();
        let id_cred_pub = base16_encode_string(&request.id_object_request.value.id_cred_pub);

        assert_eq!(id_cred_pub, "b23e360b21cb8baad1fb1f9a593d1115fc678cb9b7c1a5b5631f82e088092d79d34b6a6c8520c06c41002a666adf792f");
        assert_eq!(
            request
                .id_object_request
                .value
                .choice_ar_parameters
                .threshold
                .0,
            ar_threshold
        );
    }

    #[test]
    pub fn create_id_request_with_individual_keys() {
        let ar_threshold = 2;
        let test_data = read_test_data(ar_threshold.clone(), 0, Net::Testnet);

        let input: IdRequestInputWithKeys = IdRequestInputWithKeys {
            common:              test_data,
            prf_key:             base16_decode_string(
                "57ae5c7c108bf3eeecb34bc79a390c4d4662cefab2d95316cbdb8e68fa1632b8",
            )
            .unwrap(),
            id_cred_sec:         base16_decode_string(
                "7392eb0b4840c8a6f9314e99a8dd3e2c3663a1e615d8820851e3abd2965fab18",
            )
            .unwrap(),
            blinding_randomness: "575851a4e0558d589a57544a4a9f5ad1bd8467126c1b6767d32f633ea03380e6"
                .to_string(),
        };
        let request_string = create_id_request_with_keys_v1_aux(input).unwrap();
        let request: IdentityObjectRequestV1 = serde_json::from_str(&request_string).unwrap();
        let id_cred_pub = base16_encode_string(&request.id_object_request.value.id_cred_pub);

        assert_eq!(id_cred_pub, "b23e360b21cb8baad1fb1f9a593d1115fc678cb9b7c1a5b5631f82e088092d79d34b6a6c8520c06c41002a666adf792f");
        assert_eq!(
            request
                .id_object_request
                .value
                .choice_ar_parameters
                .threshold
                .0,
            ar_threshold
        );
    }
}
