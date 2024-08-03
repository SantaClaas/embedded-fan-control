use plotlib::page::Page;
use plotlib::repr::Plot;
use plotlib::style::LineStyle;
use plotlib::view::ContinuousView;
use rand::Rng;
use rand::rngs::ThreadRng;
use rand_distr::num_traits::Pow;

// Written by AI
fn exponential_random(random: &mut ThreadRng, max_value: f64, rate: f64) -> usize {
    let exp_sample = random.gen::<f64>();
    (max_value - (-exp_sample.ln() / rate)).floor() as usize
}

fn my_exponential_random(random: &mut ThreadRng) -> usize {
    let power: f64 = random.gen_range(0.0..5.0);
    let result = 1.0 - core::f64::consts::E.pow(-power);
    let value = result * 60.0;
    value.floor() as usize
}

fn main() {
    let mut random = rand::thread_rng();

    let mut result_counts = [0u64; 60];
    let max = 1_000_000;
    let mut count = 0;
    loop {
        // let value = my_exponential_random(&mut random);
        let value = exponential_random(&mut random, 60.0, 0.1);
        // dbg!(value);

        result_counts[value] += 1;
        // let value: u64 = ((1.0 - result) * 60.0).round() as u64;
        // dbg!(value);
        // result_counts[value as usize] += 1;

        count += 1;
        if count > max {
            break;
        }
    }

    // println!("{:#?}", result_counts);

    let mut sum = 0.0;
    let mut data = [(0.0, 0.0); 60];
    for index in 0..result_counts.len() {
        let values = result_counts[index];
        let percentage = values as f64 / max as f64 * 100.0;
        println!("{}: {:.2}", index, percentage);
        sum += percentage;
        data[index] = (index as f64 + 1.0, percentage);
    }

    println!("Sum: {:.2}", sum);

    let plot = Plot::new(data.into()).line_style(LineStyle::new().colour("red"));

    let view = ContinuousView::new()
        .add(plot)
        // .add(plot2)
        .x_range(0.0, 60.0)
        .y_range(0.0, 20.0);

    Page::single(&view).save("./exponential.svg").unwrap();

    // let max = 50;
    // let mut count = 0;
    // let mut result_counts = [0u64; 61];›
    // loop {
    //     let value: f64 = random.sample(rand_distr::Exp1);
    //     // let value = 1.0 - value;
    //     // let value = value * 60.0;
    //     // let value = value.round();
    //     dbg!(value);
    //     // result_counts[value] += 1;
    //
    //     count += 1;
    //     if count > max {
    //         break;
    //     }
    // }

    // println!("{:#?}", result_counts);

    // let mut max: f64 = 0.0;
    // loop {
    //     let value: f64 = random.sample(rand_distr::Exp1);
    //     if value > max {
    //         max = value;
    //         println!("New max: {}", max);
    //     }
    // }
}
