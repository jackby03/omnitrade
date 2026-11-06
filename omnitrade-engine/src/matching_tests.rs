use super::*;
use omnitrade_core::TimeInForce;

fn setup_engine() -> MatchingEngine {
    let fee_schedule =
        FeeSchedule::new(0.001, 0.002, "USDT").expect("fee schedule creation should succeed");
    let slippage_model = SlippageModel::new(10.0).expect("slippage model should succeed");
    MatchingEngine::new(fee_schedule, slippage_model)
}

fn create_test_order(
    id: u64,
    side: Side,
    order_type: OrderType,
    price: f64,
    quantity: f64,
) -> Order {
    Order {
        id,
        symbol: "BTCUSDT".to_string(),
        side,
        order_type,
        time_in_force: TimeInForce::GTC,
        price,
        stop_price: 0.0,
        quantity,
        filled_quantity: 0.0,
        status: OrderStatus::New,
        created_at_ms: 1000,
        updated_at_ms: 1000,
        trailing_highest_price: None,
    }
}

#[test]
fn market_buy_order_filled_at_tick_price_plus_slippage() {
    let mut engine = setup_engine();
    let order = create_test_order(1, Side::Buy, OrderType::Market, 0.0, 1.0);
    let mut pending = vec![order];
    let tick = Tick::new(2000, 50_000.0, 1.0, false);

    let fills = engine.process_tick(&tick, &mut pending);

    assert_eq!(fills.len(), 1);
    let fill = &fills[0];
    assert!((fill.fill_price - 50_050.0).abs() < 1e-6);
    assert_eq!(fill.fill_quantity, 1.0);
    assert!(!fill.is_maker);
    assert!(pending.is_empty());
}

#[test]
fn stop_loss_triggers_on_candle_low() {
    let mut engine = setup_engine();
    let mut order = create_test_order(
        1,
        Side::Sell,
        OrderType::StopLoss {
            trigger_price: 49_000.0,
            limit_price: None,
        },
        0.0,
        1.0,
    );
    order.trailing_highest_price = None;
    let mut pending = vec![order];

    let candle = Candle::new(2000, 50_000.0, 50_500.0, 48_500.0, 49_500.0, 1000.0);
    let fills = engine.process_candle(&candle, &mut pending);

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].fill_type, FillType::StopLoss);
    assert!(pending.is_empty());
}

#[test]
fn take_profit_triggers_on_candle_high() {
    let mut engine = setup_engine();
    let mut order = create_test_order(
        1,
        Side::Buy,
        OrderType::TakeProfit {
            trigger_price: 51_000.0,
            limit_price: None,
        },
        0.0,
        1.0,
    );
    order.trailing_highest_price = None;
    let mut pending = vec![order];

    let candle = Candle::new(2000, 50_000.0, 51_500.0, 49_500.0, 50_500.0, 1000.0);
    let fills = engine.process_candle(&candle, &mut pending);

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].fill_type, FillType::TakeProfit);
    assert!(pending.is_empty());
}

#[test]
fn trailing_stop_initializes_highest_on_first_candle() {
    let mut engine = setup_engine();
    let order = create_test_order(
        1,
        Side::Sell,
        OrderType::TrailingStop { percent: 0.05 },
        0.0,
        1.0,
    );
    assert_eq!(order.trailing_highest_price, None);
    let mut pending = vec![order];

    // First candle: highest should be initialized to candle.high
    let candle1 = Candle::new(2000, 50_000.0, 51_000.0, 50_000.0, 50_500.0, 1000.0);
    let fills1 = engine.process_candle(&candle1, &mut pending);
    assert!(fills1.is_empty());
    assert_eq!(pending[0].trailing_highest_price, Some(51_000.0));
}

#[test]
fn trailing_stop_updates_highest_and_triggers() {
    let mut engine = setup_engine();
    let order = create_test_order(
        1,
        Side::Sell,
        OrderType::TrailingStop { percent: 0.05 },
        0.0,
        1.0,
    );
    let mut pending = vec![order];

    // First candle: price goes up, highest = 51_000
    let candle1 = Candle::new(2000, 50_000.0, 51_000.0, 50_000.0, 50_500.0, 1000.0);
    let fills1 = engine.process_candle(&candle1, &mut pending);
    assert!(fills1.is_empty());
    assert_eq!(pending[0].trailing_highest_price, Some(51_000.0));

    // Second candle: price drops 6% below highest (threshold = 51_000 * 0.95 = 48_450)
    let candle2 = Candle::new(3000, 50_500.0, 50_600.0, 48_000.0, 48_100.0, 1000.0);
    let fills2 = engine.process_candle(&candle2, &mut pending);
    assert_eq!(fills2.len(), 1);
    assert_eq!(fills2[0].fill_type, FillType::TrailingStop);
}

#[test]
fn limit_buy_order_triggers_when_tick_price_less_than_or_equal_to_limit() {
    let mut engine = setup_engine();
    let order = create_test_order(3, Side::Buy, OrderType::Limit, 50_000.0, 1.0);
    let mut pending = vec![order];

    let tick_high = Tick::new(2000, 50_100.0, 1.0, false);
    let tick_equal = Tick::new(2001, 50_000.0, 1.0, false);

    let fills_high = engine.process_tick(&tick_high, &mut pending);
    assert!(fills_high.is_empty());
    assert_eq!(pending.len(), 1);

    let fills_equal = engine.process_tick(&tick_equal, &mut pending);
    assert_eq!(fills_equal.len(), 1);
    let fill = &fills_equal[0];
    assert!((fill.fill_price - 50_050.0).abs() < 1e-6);
    assert!(fill.is_maker);
    assert!(pending.is_empty());
}

#[test]
fn zero_remaining_quantity_marks_order_filled() {
    let mut engine = setup_engine();
    let mut order = create_test_order(1, Side::Buy, OrderType::Market, 0.0, 1.0);
    order.filled_quantity = 1.0; // Fully filled
    order.status = OrderStatus::PartiallyFilled;
    let mut pending = vec![order];

    let tick = Tick::new(2000, 50_000.0, 1.0, false);
    let fills = engine.process_tick(&tick, &mut pending);

    // No new fill generated (order already filled)
    assert!(fills.is_empty());
    // Order should have been marked as Filled, but then removed from pending_orders
    // because retain() only keeps active orders (New, PartiallyFilled)
    assert!(
        pending.is_empty(),
        "Filled orders are removed from pending_orders"
    );
}

#[test]
fn zero_quantity_order_rejected() {
    let engine = setup_engine();
    let zero_qty = create_test_order(1, Side::Buy, OrderType::Market, 0.0, 0.0);
    assert!(engine.submit_order(&zero_qty).is_err());
}

#[test]
fn submit_accepts_stop_loss_with_trigger_price_and_zero_stop_price() {
    let engine = setup_engine();
    let order = create_test_order(
        1,
        Side::Sell,
        OrderType::StopLoss {
            trigger_price: 49_000.0,
            limit_price: None,
        },
        0.0,
        1.0,
    );
    assert!(engine.submit_order(&order).is_ok());
}

#[test]
fn submit_rejects_non_positive_or_nan_trigger_price() {
    let engine = setup_engine();
    for trigger in [0.0, -1.0, f64::NAN] {
        let sl = create_test_order(
            1,
            Side::Sell,
            OrderType::StopLoss {
                trigger_price: trigger,
                limit_price: None,
            },
            0.0,
            1.0,
        );
        let tp = create_test_order(
            2,
            Side::Sell,
            OrderType::TakeProfit {
                trigger_price: trigger,
                limit_price: None,
            },
            0.0,
            1.0,
        );
        assert!(engine.submit_order(&sl).is_err());
        assert!(engine.submit_order(&tp).is_err());
    }
}

#[test]
fn buy_trailing_stop_tracks_low_and_triggers_on_rise() {
    let mut engine = setup_engine();
    let order = create_test_order(
        1,
        Side::Buy,
        OrderType::TrailingStop { percent: 0.05 },
        0.0,
        1.0,
    );
    let mut pending = vec![order];

    // Erste Kerze: Tief 100, Hoch 103 -> Schwelle 105, kein Auslöser.
    let c1 = Candle::new(1000, 102.0, 103.0, 100.0, 101.0, 10.0);
    assert!(engine.process_candle(&c1, &mut pending).is_empty());
    // Zweite Kerze: neues Tief 90 -> Schwelle 94,5; Hoch 94 reicht nicht.
    let c2 = Candle::new(2000, 95.0, 94.0, 90.0, 92.0, 10.0);
    assert!(engine.process_candle(&c2, &mut pending).is_empty());
    // Dritte Kerze: Hoch 95 >= 94,5 -> löst aus.
    let c3 = Candle::new(3000, 92.0, 95.0, 91.0, 94.0, 10.0);
    let fills = engine.process_candle(&c3, &mut pending);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].fill_type, FillType::TrailingStop);
}

#[test]
fn buy_side_stop_loss_and_take_profit_mirror_sell_side() {
    let mut engine = setup_engine();
    let sl = create_test_order(
        1,
        Side::Buy,
        OrderType::StopLoss {
            trigger_price: 105.0,
            limit_price: None,
        },
        0.0,
        1.0,
    );
    let tp = create_test_order(
        2,
        Side::Buy,
        OrderType::TakeProfit {
            trigger_price: 90.0,
            limit_price: None,
        },
        0.0,
        1.0,
    );
    let mut pending = vec![sl, tp];

    // Hoch 106 löst nur den Buy-SL aus, Tief 95 nicht den Buy-TP.
    let c1 = Candle::new(1000, 100.0, 106.0, 95.0, 100.0, 10.0);
    let fills = engine.process_candle(&c1, &mut pending);
    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].fill_type, FillType::StopLoss);
}
