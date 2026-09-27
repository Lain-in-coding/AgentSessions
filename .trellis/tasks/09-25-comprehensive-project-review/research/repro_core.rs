use app::{App, AppRequest, AppResponse, ResponseBudget};
use domain::{IdKind, StableId};
use ports::{CatalogStore, NoResumeClaims, RetrievalMode, SearchFacets, SearchFilters, SemanticIndex};
use sqlite::SqliteStore;
use serde_json::json;

fn seed(rows: &[(&str, &str, [f32; 2])]) -> SqliteStore {
    let store = SqliteStore::open_in_memory().unwrap();
    store.set_semantic_model("review-model");
    let entries: Vec<_> = rows.iter().map(|(id, role, _)| (
        StableId::native(IdKind::Message, id),
        json!({"role":role,"text":"needle","timestamp":"2026-01-01T00:00:00Z"}).to_string().into_bytes(),
        "needle".to_owned(),
    )).collect();
    store.commit_batch(&entries).unwrap();
    for (id, _, vector) in rows {
        store.index_embedding(&StableId::native(IdKind::Message, id), vector).unwrap();
    }
    store
}

fn search(store: &SqliteStore, mode: RetrievalMode, limit: usize, cursor: Option<String>, filters: SearchFilters) -> Result<AppResponse, app::AppError> {
    App::with_resume_semantic(store, store, NoResumeClaims, store).handle(AppRequest::Search {
        query: "needle".into(), filters, facets: SearchFacets::default(), limit, cursor,
        budget: ResponseBudget::default(), include_system: false, group_by_session: false,
        mode, query_embedding: Some(vec![1.0, 0.0]),
    })
}

fn unpack(response: AppResponse) -> (Vec<String>, Option<String>) {
    let AppResponse::Search { hits, next_cursor, .. } = response else { panic!("search expected") };
    (hits.into_iter().map(|h| h.id.as_str().to_owned()).collect(), next_cursor)
}

fn main() {
    let panic = std::panic::catch_unwind(|| ports::redact::redact_text(&format!("AKIA{}", "中".repeat(6)))).is_err();
    println!("REPRO {}", json!({"case":"unicode_redaction_panic","panic":panic}));
    let extreme = std::panic::catch_unwind(|| app::parse_search_instant("9223372036854775807-12-31T00:00:00Z")).is_err();
    println!("REPRO {}", json!({"case":"extreme_year_panic","panic":extreme}));
    println!("REPRO {}", json!({"case":"invalid_negative_clock_accepted","accepted":app::parse_search_instant("2026-01-01T00:-1:00Z").is_some()}));
    let a = StableId::native(IdKind::Message, "a\0b");
    let b = StableId::native(IdKind::Message, "ab");
    println!("REPRO {}", json!({"case":"native_identity_collision","equal":a==b,"wire":a.as_str()}));

    let store = seed(&[("a", "user", [0.8, 0.6]), ("b", "user", [0.1, 0.99]), ("c", "user", [1.0, 0.0])]);
    let filters = SearchFilters { since: app::parse_search_instant("2050-01-01T00:00:00Z"), ..Default::default() };
    let lexical = unpack(search(&store, RetrievalMode::Lexical, 10, None, filters.clone()).unwrap()).0;
    let semantic = unpack(search(&store, RetrievalMode::Semantic, 10, None, filters).unwrap()).0;
    println!("REPRO {}", json!({"case":"semantic_ignores_time_filter","lexical":lexical,"semantic":semantic}));

    let (first, token) = unpack(search(&store, RetrievalMode::Lexical, 1, None, SearchFilters::default()).unwrap());
    let switched = search(&store, RetrievalMode::Semantic, 1, token, SearchFilters::default());
    let accepted = switched.is_ok();
    let second = switched.map(unpack).map(|p|p.0).unwrap_or_default();
    println!("REPRO {}", json!({"case":"cursor_accepts_changed_mode","accepted":accepted,"first":first,"second":second}));

    let dead = StableId::native(IdKind::Message, "c");
    let pending = store.begin_index_batch(&[], std::slice::from_ref(&dead)).unwrap();
    store.commit_index_batch(&pending, &[], std::slice::from_ref(&dead)).unwrap();
    let ghost = store.query_semantic(&[1.0, 0.0], 10).unwrap().iter().any(|h| h.id == dead);
    println!("REPRO {}", json!({"case":"deleted_vector_ghost","catalog_deleted":store.get(&dead).unwrap().is_none(),"semantic_still_returns":ghost}));

    let noise = seed(&[("a", "user", [1.0,0.0]),("b", "system", [0.99,0.1]),("c", "system", [0.98,0.2]),("d", "user", [0.97,0.3])]);
    let (first, token) = unpack(search(&noise, RetrievalMode::Semantic, 1, None, SearchFilters::default()).unwrap());
    let (second, next) = unpack(search(&noise, RetrievalMode::Semantic, 1, token, SearchFilters::default()).unwrap());
    let all = unpack(search(&noise, RetrievalMode::Semantic, 10, None, SearchFilters::default()).unwrap()).0;
    println!("REPRO {}", json!({"case":"semantic_noise_ends_pagination_early","first":first,"second":second,"has_next":next.is_some(),"all":all}));

    let nan = seed(&[("bad", "user", [f32::NAN, 1.0])]);
    let nonfinite = nan.query_semantic(&[1.0,0.0], 10).unwrap().iter().any(|h| !h.score.is_finite());
    println!("REPRO {}", json!({"case":"nonfinite_semantic_score","nonfinite":nonfinite}));
}
