use std::collections::HashMap;

use json_patch::{
    patch,
    Patch,
};
use kube::api::TypeMeta;
use kube::core::admission::{
    AdmissionRequest,
    AdmissionResponse,
    AdmissionReview,
    Operation,
};
use kube::core::{
    GroupVersionKind,
    GroupVersionResource,
};
use kube::ResourceExt;
use mockall::predicate;
use rocket::serde::json::Json;
use simkube::testutils::fake::make_fake_apiserver;
use simkube::testutils::*;
use tracing_test::traced_test;

use super::*;

#[fixture]
fn ctx(
    test_pod: corev1::Pod,
    #[default(vec![])] pod_owners: Vec<metav1::OwnerReference>,
    #[default(MockTraceStore::new())] store: MockTraceStore,
) -> DriverContext {
    let (_, client) = make_fake_apiserver();
    let mut owners = HashMap::new();
    owners.insert(test_pod.namespaced_name(), pod_owners);

    DriverContext {
        name: TEST_SIM_NAME.into(),
        sim_root: TEST_SIM_ROOT_NAME.into(),
        virtual_ns_prefix: "virtual".into(),
        owners_cache: Arc::new(Mutex::new(OwnersCache::new_from_parts(ApiSet::new(client), owners))),
        store: Arc::new(store),
    }
}

#[fixture]
fn adm_req(test_pod: corev1::Pod) -> AdmissionRequest<corev1::Pod> {
    let gvr = GroupVersionResource::gvr("".into(), "v1".into(), "pods".into());
    let gvk = GroupVersionKind::gvk("".into(), "v1".into(), "Pod".into());
    AdmissionRequest {
        types: TypeMeta { api_version: "v1".into(), kind: "Pod".into() },
        uid: "12345-12345".into(),
        kind: gvk,
        resource: gvr,
        sub_resource: None,
        request_kind: None,
        request_resource: None,
        request_sub_resource: None,
        name: test_pod.name_any(),
        namespace: Some(test_pod.namespace().unwrap()),
        operation: Operation::Create,
        user_info: Default::default(),
        object: Some(test_pod.clone()),
        old_object: None,
        dry_run: false,
        options: None,
    }
}

#[fixture]
fn adm_rev(adm_req: AdmissionRequest<corev1::Pod>) -> AdmissionReview<corev1::Pod> {
    AdmissionReview {
        types: Default::default(),
        request: Some(adm_req),
        response: None,
    }
}

#[fixture]
fn adm_resp(adm_req: AdmissionRequest<corev1::Pod>) -> AdmissionResponse {
    AdmissionResponse::from(&adm_req)
}

#[rstest]
#[traced_test]
#[tokio::test]
async fn test_handler_invalid_review(ctx: DriverContext) {
    let adm_rev = AdmissionReview {
        types: Default::default(),
        request: None,
        response: None,
    };
    let resp = handler(rocket::State::from(&ctx), Json(adm_rev), rocket::State::from(&MutationData::new())).await;
    assert!(!resp.0.response.unwrap().allowed);
}

#[rstest]
#[traced_test]
#[tokio::test]
async fn test_handler_bad_response(mut test_pod: corev1::Pod, mut adm_rev: AdmissionReview<corev1::Pod>) {
    let owner = metav1::OwnerReference {
        name: TEST_SIM_ROOT_NAME.into(),
        ..Default::default()
    };
    let ctx = ctx(test_pod.clone(), vec![owner.clone()], MockTraceStore::new());
    test_pod.owner_references_mut().push(owner);
    test_pod.spec = None;

    *adm_rev.request.as_mut().unwrap().object.as_mut().unwrap() = test_pod;
    let resp = handler(rocket::State::from(&ctx), Json(adm_rev), rocket::State::from(&MutationData::new())).await;
    assert!(!resp.0.response.unwrap().allowed);
}

#[rstest]
#[tokio::test]
async fn test_mutate_pod_not_owned_by_sim(mut test_pod: corev1::Pod, mut adm_resp: AdmissionResponse) {
    let owner = metav1::OwnerReference { name: "foo".into(), ..Default::default() };
    let ctx = ctx(test_pod.clone(), vec![owner.clone()], MockTraceStore::new());
    test_pod.owner_references_mut().push(owner);
    adm_resp = mutate_pod(&ctx, adm_resp, &test_pod, &MutationData::new()).await.unwrap();
    assert_eq!(adm_resp.patch, None);
}

#[rstest]
#[tokio::test]
async fn test_mutate_pod(mut test_pod: corev1::Pod, mut adm_resp: AdmissionResponse) {
    test_pod
        .annotations_mut()
        .insert(ORIG_NAMESPACE_ANNOTATION_KEY.into(), TEST_NAMESPACE.into());
    let root = metav1::OwnerReference {
        name: TEST_SIM_ROOT_NAME.into(),
        ..Default::default()
    };
    let depl = metav1::OwnerReference { name: TEST_DEPLOYMENT.into(), ..Default::default() };

    let owner_ns_name = format!("{TEST_NAMESPACE}/{TEST_DEPLOYMENT}");
    let mut store = MockTraceStore::new();
    let _ = store
        .expect_lookup_pod_lifecycle()
        .with(predicate::always(), predicate::eq(EMPTY_POD_SPEC_HASH), predicate::eq(0))
        .returning(|_, _, _| PodLifecycleData::Finished(1, 2))
        .once();
    let _ = store.expect_has_obj().returning(move |o| o == owner_ns_name);

    let ctx = ctx(test_pod.clone(), vec![root.clone(), depl.clone()], store);

    adm_resp = mutate_pod(&ctx, adm_resp, &test_pod, &MutationData::new()).await.unwrap();
    let mut json_pod = serde_json::to_value(&test_pod).unwrap();
    let pod_patch: Patch = serde_json::from_slice(&adm_resp.patch.unwrap()).unwrap();
    patch(&mut json_pod, &pod_patch).unwrap();
}
