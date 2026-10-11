// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! BERT encoder with a masked language model head that only decodes the
//! rows it is asked for.
//!
//! candle ships `BertForMaskedLM`, but it projects every token onto the
//! 21128-entry vocabulary and expects an untied decoder. Scoring needs one
//! or a few rows, and the decoder dominates the cost once a batch holds many
//! sequences, so this head gathers the wanted hidden states first. It also
//! falls back to the word embeddings and to `cls.predictions.bias` when a
//! checkpoint ties the decoder, as google-bert and macbert4csc do.

use candle_core::{D, DType, Device, IndexOp, Module, Tensor};
use candle_nn::{LayerNorm, Linear, VarBuilder, layer_norm, linear};
use candle_transformers::models::bert::{BertModel, Config};

use crate::Error;

/// A BERT encoder with the masked language model head of the checkpoint.
pub(crate) struct MaskedLm {
    bert: BertModel,
    /// Dense layer of the head's transform.
    dense: Linear,
    /// Layer norm of the head's transform.
    norm: LayerNorm,
    /// Output projection, one row per vocabulary entry.
    decoder: Tensor,
    bias: Tensor,
    device: Device,
}

impl MaskedLm {
    /// Loads the encoder and head from `vb`, using `config` for the shapes.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Model`] when a tensor is missing or has the wrong
    /// shape.
    pub(crate) fn load(vb: VarBuilder, config: &Config) -> Result<Self, Error> {
        let bert = BertModel::load(vb.pp("bert"), config)?;
        let head = vb.pp("cls.predictions");
        let dense = linear(
            config.hidden_size,
            config.hidden_size,
            head.pp("transform.dense"),
        )?;
        let norm = layer_norm(
            config.hidden_size,
            config.layer_norm_eps,
            head.pp("transform.LayerNorm"),
        )?;
        let shape = (config.vocab_size, config.hidden_size);
        // A tied decoder shares the word embeddings and may not be stored
        // as a tensor of its own.
        let decoder = if head.contains_tensor("decoder.weight") {
            head.get(shape, "decoder.weight")?
        } else {
            vb.get(shape, "bert.embeddings.word_embeddings.weight")?
        };
        let bias = if head.contains_tensor("bias") {
            head.get(config.vocab_size, "bias")?
        } else {
            head.get(config.vocab_size, "decoder.bias")?
        };
        Ok(Self {
            bert,
            dense,
            norm,
            decoder,
            bias,
            device: vb.device().clone(),
        })
    }

    /// Runs `batch`, a list of equally long token sequences, and returns the
    /// vocabulary distribution at each `(sequence, token)` in `rows`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Model`] when the forward pass fails.
    pub(crate) fn probabilities(
        &self,
        batch: &[Vec<u32>],
        rows: &[(usize, usize)],
    ) -> Result<Vec<Vec<f32>>, Error> {
        let len = batch.first().map_or(0, Vec::len);
        let flat: Vec<u32> = batch.iter().flatten().copied().collect();
        let ids = Tensor::from_vec(flat, (batch.len(), len), &self.device)?;
        let types = ids.zeros_like()?;
        let hidden = self.bert.forward(&ids, &types, None)?;
        let hidden = hidden.reshape((batch.len() * len, ()))?;

        // Row of each wanted token in the flattened `(sequence, token)` axis.
        let picks: Vec<u32> = rows
            .iter()
            .map(|&(seq, tok)| u32::try_from(seq * len + tok).unwrap_or(u32::MAX))
            .collect();
        let picks = Tensor::new(picks.as_slice(), &self.device)?;
        let h = hidden.index_select(&picks, 0)?;
        let h = self.norm.forward(&self.dense.forward(&h)?.gelu_erf()?)?;
        let logits = h.matmul(&self.decoder.t()?)?.broadcast_add(&self.bias)?;
        let probs = candle_nn::ops::softmax(&logits, D::Minus1)?.to_dtype(DType::F32)?;
        (0..rows.len())
            .map(|i| Ok(probs.i(i)?.to_vec1::<f32>()?))
            .collect()
    }
}
