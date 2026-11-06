use super::*;
use async_trait::async_trait;
use omnitrade_core::{Order, OrderType, TimeInForce};
use omnitrade_exchange::{Account, AccountBalance, ExchangeError};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Book {
    next_id: u64,
    orders: HashMap<u64, Order>,
    reject_sells: bool,
}

#[derive(Clone, Default)]
struct MockTrading {
    book: Arc<Mutex<Book>>,
}

impl MockTrading {
    fn set_status(&self, id: u64, status: OrderStatus) {
        let mut b = self.book.lock().expect("book");
        if let Some(o) = b.orders.get_mut(&id) {
            o.status = status;
            if status == OrderStatus::Filled {
                o.filled_quantity = o.quantity;
            }
        }
    }

    fn open(&self) -> Vec<Order> {
        let b = self.book.lock().expect("book");
        let mut v: Vec<Order> = b
            .orders
            .values()
            .filter(|o| matches!(o.status, OrderStatus::New | OrderStatus::PartiallyFilled))
            .cloned()
            .collect();
        v.sort_by_key(|o| o.id);
        v
    }
}

#[async_trait]
impl ExchangeTrading for MockTrading {
    async fn place_order(&self, req: &OrderRequest) -> Result<Order, ExchangeError> {
        let mut b = self.book.lock().expect("book");
        if b.reject_sells && req.side == Side::Sell {
            return Err(ExchangeError::ApiError("rejected".into()));
        }
        b.next_id += 1;
        let id = b.next_id;
        let order = Order {
            id,
            symbol: req.symbol.clone(),
            side: req.side,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GTC,
            price: req.price.unwrap_or(0.0),
            stop_price: 0.0,
            quantity: req.quantity,
            filled_quantity: 0.0,
            status: OrderStatus::New,
            created_at_ms: 0,
            updated_at_ms: 0,
            trailing_highest_price: None,
        };
        b.orders.insert(id, order.clone());
        Ok(order)
    }

    async fn cancel_order(&self, order_id: &str, _symbol: &str) -> Result<(), ExchangeError> {
        let id: u64 = order_id.parse().expect("id");
        self.set_status(id, OrderStatus::Cancelled);
        Ok(())
    }

    async fn get_open_orders(&self) -> Result<Vec<Order>, ExchangeError> {
        Ok(self.open())
    }

    async fn get_balance(&self) -> Result<Account, ExchangeError> {
        Ok(Account {
            balances: HashMap::from([(
                "USDT".to_string(),
                AccountBalance {
                    free: 1000.0,
                    locked: 0.0,
                },
            )]),
        })
    }

    async fn get_order(
        &self,
        order_id: &str,
        _symbol: &str,
    ) -> Result<Option<Order>, ExchangeError> {
        let id: u64 = order_id.parse().expect("id");
        Ok(self.book.lock().expect("book").orders.get(&id).cloned())
    }
}

fn kill_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("omnitrade-grid-{}-{name}.kill", std::process::id()))
}

fn grid() -> GridConfig {
    // Drei Kauf-Stufen: 90.0, 81.0, 72.9; Menge je Stufe 0.01.
    GridConfig::new(3, 0.1, 0.01, 100.0, 1.0).expect("grid")
}

async fn run(
    trading: &MockTrading,
    grid: &GridConfig,
    kill: &Path,
    driver: impl std::future::Future<Output = ()>,
) -> Result<(), LiveLoopError> {
    let cfg = GridLiveConfig {
        grid,
        symbol: "BTCUSDT",
        initial_capital: 1000.0,
        kill_file: kill,
        poll_interval: Duration::from_millis(5),
        price_decimals: 2,
        qty_decimals: 5,
    };
    let mut guard = RiskGuard::testnet();
    let (res, ()) = tokio::join!(grid_loop(&cfg, trading.clone(), &mut guard), driver);
    res
}

async fn pause() {
    tokio::time::sleep(Duration::from_millis(40)).await;
}

#[tokio::test]
async fn places_buy_ladder_and_cancels_on_kill() {
    let t = MockTrading::default();
    let kill = kill_path("ladder");
    let g = grid();
    let k = kill.clone();
    let t2 = t.clone();
    run(&t, &g, &kill, async move {
        pause().await;
        assert_eq!(t2.open().len(), 3);
        kill_switch::engage(&k).expect("engage");
        pause().await;
    })
    .await
    .expect("grid ends on kill");
    let _ = std::fs::remove_file(&kill);

    assert!(t.open().is_empty(), "Kill storniert alle Stufen");
    let prices: Vec<f64> = {
        let b = t.book.lock().expect("book");
        let mut v: Vec<(u64, f64)> = b.orders.values().map(|o| (o.id, o.price)).collect();
        v.sort_by_key(|(id, _)| *id);
        v.into_iter().map(|(_, p)| p).collect()
    };
    assert_eq!(prices, vec![90.0, 81.0, 72.9]);
}

#[tokio::test]
async fn filled_buy_becomes_sell_one_level_up_and_filled_sell_becomes_buy() {
    let t = MockTrading::default();
    let kill = kill_path("counter");
    let g = grid();
    let (k, t2) = (kill.clone(), t.clone());
    run(&t, &g, &kill, async move {
        pause().await;
        t2.set_status(1, OrderStatus::Filled); // Kauf @ 90
        pause().await;
        let open = t2.open();
        let sell = open
            .iter()
            .find(|o| o.side == Side::Sell)
            .expect("Gegenorder");
        assert!((sell.price - 99.0).abs() < 1e-9); // 90 * 1.1
        assert!((sell.quantity - 0.01).abs() < 1e-9);
        let sell_id = sell.id;
        t2.set_status(sell_id, OrderStatus::Filled);
        pause().await;
        let open = t2.open();
        assert!(open.iter().all(|o| o.side == Side::Buy));
        assert!(open
            .iter()
            .any(|o| (o.price - 89.1).abs() < 1e-9 && o.id > sell_id));
        kill_switch::engage(&k).expect("engage");
        pause().await;
    })
    .await
    .expect("grid");
    let _ = std::fs::remove_file(&kill);
}

#[tokio::test]
async fn externally_cancelled_level_is_dropped_without_counter_order() {
    let t = MockTrading::default();
    let kill = kill_path("external");
    let g = grid();
    let (k, t2) = (kill.clone(), t.clone());
    run(&t, &g, &kill, async move {
        pause().await;
        t2.set_status(2, OrderStatus::Cancelled);
        pause().await;
        let open = t2.open();
        assert_eq!(open.len(), 2);
        assert!(open.iter().all(|o| o.side == Side::Buy));
        kill_switch::engage(&k).expect("engage");
        pause().await;
    })
    .await
    .expect("grid");
    let _ = std::fs::remove_file(&kill);
}

#[tokio::test]
async fn rejected_counter_order_leaves_gap_but_grid_keeps_running() {
    let t = MockTrading::default();
    t.book.lock().expect("book").reject_sells = true;
    let kill = kill_path("reject");
    let g = grid();
    let (k, t2) = (kill.clone(), t.clone());
    run(&t, &g, &kill, async move {
        pause().await;
        t2.set_status(1, OrderStatus::Filled);
        pause().await;
        assert_eq!(
            t2.open().len(),
            2,
            "Gegenorder abgelehnt, Rest läuft weiter"
        );
        kill_switch::engage(&k).expect("engage");
        pause().await;
    })
    .await
    .expect("grid");
    let _ = std::fs::remove_file(&kill);
}

#[tokio::test]
async fn leverage_other_than_one_is_rejected() {
    let t = MockTrading::default();
    let kill = kill_path("leverage");
    let g = GridConfig::new(3, 0.1, 0.01, 100.0, 2.0).expect("grid");
    let res = run(&t, &g, &kill, async {}).await;
    assert!(matches!(res, Err(LiveLoopError::ExchangeError(_))));
    assert!(t.open().is_empty());
}

#[test]
fn counter_level_rounds_price_and_floors_quantity() {
    let filled = Level {
        side: Side::Buy,
        price: 72.9,
        qty: 0.01,
    };
    let c = counter_level(filled, 0.012_349_9, 0.1, 2, 5);
    assert_eq!(c.side, Side::Sell);
    assert!((c.price - 80.19).abs() < 1e-9);
    assert!((c.qty - 0.012_34).abs() < 1e-12);
}
