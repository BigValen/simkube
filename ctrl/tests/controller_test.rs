use std::env;

use kube::runtime::controller::Action;
use serde_json::json;
use simkube::metrics::api::*;
use simkube::testutils::fake::*;
use simkube::testutils::*;
use tracing_test::traced_test;

use super::controller::*;
use super::*;
use crate::metrics::objects::*;

#[fixture]
fn sim() -> Simulation {
    Simulation {
        metadata: metav1::ObjectMeta {
            name: Some(TEST_SIM_NAME.into()),
            uid: Some("1234-asdf".into()),
            ..Default::default()
        },
        spec: SimulationSpec {
            driver_namespace: TEST_NAMESPACE.into(),
            trace: "file:///foo/bar".into(),
            ..Default::default()
        },
        status: Default::default(),
    }
}

#[fixture]
fn root() -> SimulationRoot {
    SimulationRoot {
        metadata: metav1::ObjectMeta {
            name: Some(format!("sk-{TEST_SIM_NAME}-root")),
            uid: Some("qwerty-5678".into()),
            ..Default::default()
        },
        spec: SimulationRootSpec {},
    }
}

#[fixture]
fn opts() -> Options {
    Options {
        driver_image: "driver:latest".into(),
        driver_port: 1234,
        use_cert_manager: false,
        cert_manager_issuer: "".into(),
        verbosity: "info".into(),
    }
}

#[rstest]
#[tokio::test]
async fn test_fetch_driver_status_no_driver(sim: Simulation, opts: Options) {
    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);

    let driver_name = ctx.driver_name.clone();
    fake_apiserver
        .handle_not_found(&format!("/apis/batch/v1/namespaces/{TEST_NAMESPACE}/jobs/{driver_name}"))
        .build();
    assert_eq!(SimulationState::Initializing, fetch_driver_status(&ctx).await.unwrap().0);
    fake_apiserver.assert();
}

#[rstest]
#[tokio::test]
async fn test_fetch_driver_status_driver_no_status(sim: Simulation, opts: Options) {
    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);

    let driver_name = ctx.driver_name.clone();
    fake_apiserver
        .handle(move |when, then| {
            when.path(format!("/apis/batch/v1/namespaces/{TEST_NAMESPACE}/jobs/{driver_name}"));
            then.json_body(json!({}));
        })
        .build();
    assert_eq!(SimulationState::Running, fetch_driver_status(&ctx).await.unwrap().0);
    fake_apiserver.assert();
}

#[rstest]
#[tokio::test]
async fn test_fetch_driver_status_driver_running(sim: Simulation, opts: Options) {
    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);

    let driver_name = ctx.driver_name.clone();
    fake_apiserver
        .handle(move |when, then| {
            when.path(format!("/apis/batch/v1/namespaces/{TEST_NAMESPACE}/jobs/{driver_name}"));
            then.json_body(json!({
                "status": {
                    "conditions": [{ "type": "Running" }],
                },
            }));
        })
        .build();
    assert_eq!(SimulationState::Running, fetch_driver_status(&ctx).await.unwrap().0);
    fake_apiserver.assert();
}

#[rstest]
#[case("Completed")]
#[case("Failed")]
#[tokio::test]
async fn test_fetch_driver_status_driver_finished(sim: Simulation, opts: Options, #[case] status: &'static str) {
    let expected_state = if status == "Completed" { SimulationState::Finished } else { SimulationState::Failed };

    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);

    let driver_name = ctx.driver_name.clone();
    fake_apiserver
        .handle(move |when, then| {
            when.path(format!("/apis/batch/v1/namespaces/{TEST_NAMESPACE}/jobs/{driver_name}"));
            then.json_body(json!({
                "status": {
                    "conditions": [{"type": "Running"}, { "type": status }],
                },
            }));
        })
        .build();
    assert_eq!(expected_state, fetch_driver_status(&ctx).await.unwrap().0);
    fake_apiserver.assert();
}

#[rstest]
#[tokio::test]
async fn test_setup_driver_no_ns(sim: Simulation, root: SimulationRoot, opts: Options) {
    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);
    fake_apiserver
        .handle_not_found(&format!("/api/v1/namespaces/{DEFAULT_METRICS_NS}"))
        .build();

    assert!(matches!(
        setup_driver(&ctx, &sim, &root)
            .await
            .unwrap_err()
            .downcast::<SkControllerError>()
            .unwrap(),
        SkControllerError::NamespaceNotFound(_)
    ))
}

#[rstest]
#[traced_test]
#[tokio::test]
async fn test_setup_driver_create_prom(sim: Simulation, root: SimulationRoot, opts: Options) {
    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);

    let driver_ns = ctx.driver_ns.clone();
    let prom_name = ctx.prometheus_name.clone();
    let prom_svc_name = ctx.prometheus_svc.clone();
    let driver_ns_obj = build_driver_namespace(&ctx, &sim).unwrap();
    let service_monitor_obj = build_ksm_service_monitor(KSM_SVC_MON_NAME, &sim).unwrap();
    let prom_obj = build_prometheus(&ctx.prometheus_name, KSM_SVC_MON_NAME, &sim).unwrap();
    let prom_svc_obj = build_prometheus_service(&ctx.prometheus_svc, &sim).unwrap();

    fake_apiserver
        .handle(|when, then| {
            when.method(GET).path(format!("/api/v1/namespaces/{DEFAULT_METRICS_NS}"));
            then.json_body(json!({
                "kind": "Namespace",
            }));
        })
        .handle_not_found(&format!("/api/v1/namespaces/{driver_ns}"))
        .handle(move |when, then| {
            when.method(POST).path("/api/v1/namespaces");
            then.json_body_obj(&driver_ns_obj);
        })
        .handle_not_found(&format!(
            "/apis/monitoring.coreos.com/v1/namespaces/monitoring/servicemonitors/{KSM_SVC_MON_NAME}"
        ))
        .handle(move |when, then| {
            when.method(POST)
                .path("/apis/monitoring.coreos.com/v1/namespaces/monitoring/servicemonitors");
            then.json_body_obj(&service_monitor_obj);
        })
        .handle_not_found(&format!("/apis/monitoring.coreos.com/v1/namespaces/monitoring/prometheuses/{prom_name}"))
        .handle(move |when, then| {
            when.method(POST)
                .path("/apis/monitoring.coreos.com/v1/namespaces/monitoring/prometheuses");
            then.json_body_obj(&prom_obj);
        })
        .handle_not_found(&format!("/api/v1/namespaces/monitoring/services/{prom_svc_name}"))
        .handle(move |when, then| {
            when.method(POST).path("/api/v1/namespaces/monitoring/services");
            then.json_body_obj(&prom_svc_obj);
        })
        .build();
    assert_eq!(setup_driver(&ctx, &sim, &root).await.unwrap(), Action::requeue(REQUEUE_DURATION));
    fake_apiserver.assert();
}

#[rstest]
#[case(true)]
#[case(false)]
#[traced_test]
#[tokio::test]
async fn test_setup_driver_wait_prom(sim: Simulation, root: SimulationRoot, opts: Options, #[case] ready: bool) {
    env::set_var("POD_SVC_ACCOUNT", "asdf");
    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);

    let driver_ns = ctx.driver_ns.clone();
    let prom_name = ctx.prometheus_name.clone();
    let prom_svc_name = ctx.prometheus_svc.clone();
    let driver_svc_name = ctx.driver_svc.clone();
    let webhook_name = ctx.webhook_name.clone();
    let driver_name = ctx.driver_name.clone();

    let driver_ns_obj = build_driver_namespace(&ctx, &sim).unwrap();
    let service_monitor_obj = build_ksm_service_monitor(KSM_SVC_MON_NAME, &sim).unwrap();
    let prom_obj = build_prometheus(&ctx.prometheus_name, KSM_SVC_MON_NAME, &sim).unwrap();
    let prom_svc_obj = build_prometheus_service(&ctx.prometheus_svc, &sim).unwrap();
    let driver_svc_obj = build_driver_service(&ctx, &root).unwrap();
    let webhook_obj = build_mutating_webhook(&ctx, &root).unwrap();
    let driver_obj = build_driver_job(&ctx, &sim, "".into(), &sim.spec.trace.clone()).unwrap();

    fake_apiserver
        .handle(|when, then| {
            when.method(GET).path(format!("/api/v1/namespaces/{DEFAULT_METRICS_NS}"));
            then.json_body(json!({
                "kind": "Namespace",
            }));
        })
        .handle(move |when, then| {
            when.method(GET).path(format!("/api/v1/namespaces/{driver_ns}"));
            then.json_body_obj(&driver_ns_obj);
        })
        .handle(move |when, then| {
            when.method(GET).path(format!(
                "/apis/monitoring.coreos.com/v1/namespaces/monitoring/servicemonitors/{KSM_SVC_MON_NAME}"
            ));
            then.json_body_obj(&service_monitor_obj);
        })
        .handle(move |when, then| {
            when.method(GET)
                .path(format!("/apis/monitoring.coreos.com/v1/namespaces/monitoring/prometheuses/{prom_name}"));
            let mut prom_obj = prom_obj.clone();
            if ready {
                prom_obj.status = Some(PrometheusStatus { available_replicas: 1, ..Default::default() });
            }
            then.json_body_obj(&prom_obj);
        })
        .handle(move |when, then| {
            when.method(GET)
                .path(format!("/api/v1/namespaces/monitoring/services/{prom_svc_name}"));
            then.json_body_obj(&prom_svc_obj);
        });

    if ready {
        fake_apiserver
            .handle_not_found(&format!("/api/v1/namespaces/test/services/{driver_svc_name}"))
            .handle(move |when, then| {
                when.method(POST).path("/api/v1/namespaces/test/services");
                then.json_body_obj(&driver_svc_obj);
            })
            .handle(move |when, then| {
                when.method(GET).path("/api/v1/namespaces/test/secrets");
                then.json_body(json!({
                    "kind": "SecretList",
                    "metadata": {},
                    "items": [{
                        "kind": "Secret"
                    }],
                }));
            })
            .handle_not_found(&format!(
                "/apis/admissionregistration.k8s.io/v1/mutatingwebhookconfigurations/{webhook_name}",
            ))
            .handle(move |when, then| {
                when.method(POST)
                    .path("/apis/admissionregistration.k8s.io/v1/mutatingwebhookconfigurations");
                then.json_body_obj(&webhook_obj);
            })
            .handle_not_found(&format!("/apis/batch/v1/namespaces/test/jobs/{driver_name}"))
            .handle(move |when, then| {
                when.method(POST).path("/apis/batch/v1/namespaces/test/jobs");
                then.json_body_obj(&driver_obj);
            });
    }
    fake_apiserver.build();
    let res = setup_driver(&ctx, &sim, &root).await.unwrap();
    if ready {
        assert_eq!(res, Action::await_change());
    } else {
        assert_eq!(res, Action::requeue(REQUEUE_DURATION));
    }
    fake_apiserver.assert();
}


#[rstest]
#[traced_test]
#[tokio::test]
async fn test_cleanup(sim: Simulation, opts: Options) {
    let (mut fake_apiserver, client) = make_fake_apiserver();
    let ctx = Arc::new(SimulationContext::new(client, opts)).with_sim(&sim);

    let root = ctx.root.clone();
    let prom = ctx.prometheus_name.clone();

    fake_apiserver
        .handle(move |when, then| {
            when.path(format!("/apis/simkube.io/v1/simulationroots/{root}"));
            then.json_body(status_ok());
        })
        .handle(|when, then| {
            when.path(format!(
                "/apis/monitoring.coreos.com/v1/namespaces/monitoring/servicemonitors/{KSM_SVC_MON_NAME}"
            ));
            then.json_body(status_ok());
        })
        .handle(move |when, then| {
            when.path(format!("/apis/monitoring.coreos.com/v1/namespaces/monitoring/prometheuses/{prom}"));
            then.json_body(status_ok());
        });
    fake_apiserver.build();
    cleanup(&ctx, &sim).await;

    assert!(!logs_contain("ERROR"));
    fake_apiserver.assert();
}
