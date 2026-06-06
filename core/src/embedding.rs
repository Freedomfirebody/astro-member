use anyhow::{anyhow, Result};
use fastembed::{InitOptions, TextEmbedding};
use once_cell::sync::OnceCell;
use std::path::PathBuf;

pub struct EmbeddingManager {
    cache_dir: Option<PathBuf>,
    model: OnceCell<TextEmbedding>,
}

impl EmbeddingManager {
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        Self {
            cache_dir,
            model: OnceCell::new(),
        }
    }

    fn get_model(&self) -> Result<&TextEmbedding> {
        if std::env::var("ASTRO_MEMBER_SKIP_EMBEDDING").is_ok() {
            return Err(anyhow!("Embedding model loading is disabled via ASTRO_MEMBER_SKIP_EMBEDDING env var"));
        }
        self.model.get_or_try_init(|| {
            let mut options = InitOptions::default();
            if let Some(ref cache) = self.cache_dir {
                if cache.is_file() {
                    return Err(anyhow!("Cache directory is a file: {:?}", cache));
                }
                options.cache_dir = cache.clone();
            }
            TextEmbedding::try_new(options)
                .map_err(|e| anyhow!("Failed to initialize fastembed model: {:?}", e))
        })
    }

    pub fn generate_passage_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let model = self.get_model()?;
        let embeddings = model
            .embed(vec![text], None)
            .map_err(|e| anyhow!("Failed to generate passage embedding: {:?}", e))?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No embedding returned from fastembed"))
    }

    pub fn generate_query_embedding(&self, query: &str) -> Result<Vec<f32>> {
        let model = self.get_model()?;
        let embeddings = model
            .embed(vec![query], None)
            .map_err(|e| anyhow!("Failed to generate query embedding: {:?}", e))?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("No embedding returned from fastembed"))
    }
}
