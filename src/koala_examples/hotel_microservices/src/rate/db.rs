use mongodb::bson::doc;
use mongodb::error::Result;
use mongodb::{Client, Database};
use mongodb::IndexModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomType {
    #[serde(rename = "bookableRate")]
    pub bookable_rate: f64,
    pub code: String,
    #[serde(rename = "roomDescription")]
    pub room_description: String,
    #[serde(rename = "totalRate")]
    pub total_rate: f64,
    #[serde(rename = "totalRateInclusive")]
    pub total_rate_inclusive: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatePlan {
    #[serde(rename = "hotelId")]
    pub hotel_id: String,
    pub code: String,
    #[serde(rename = "inDate")]
    pub in_date: String,
    #[serde(rename = "outDate")]
    pub out_date: String,
    #[serde(rename = "roomType")]
    pub room_type: RoomType,
}

pub async fn initialize_database(uri: impl AsRef<str>) -> Result<Database> {
    let client = Client::with_uri_str(uri).await?;

    log::info!("New session successful...");

    log::info!("Generating test data...");
    let db = client.database("rate-db");
    let collections = db.collection::<RatePlan>("inventory");

    let count = collections.count_documents(doc! { "hotelId": "1" }, None).await?;
    if count == 0 {
        let plan = RatePlan {
            hotel_id: "1".to_string(),
            code: "RACK".to_string(),
            in_date: "2015-04-09".to_string(),
            out_date: "2015-04-10".to_string(),
            room_type: RoomType {
                bookable_rate: 109.00,
                code: "KNG".to_string(),
                room_description: "King sized bed".to_string(),
                total_rate: 109.00,
                total_rate_inclusive: 123.17,
            },
        };
        collections.insert_one(plan, None).await?;
    }

    let count = collections.count_documents(doc! { "hotelId": "2" }, None).await?;
    if count == 0 {
        let plan = RatePlan {
            hotel_id: "2".to_string(),
            code: "RACK".to_string(),
            in_date: "2015-04-09".to_string(),
            out_date: "2015-04-10".to_string(),
            room_type: RoomType {
                bookable_rate: 139.00,
                code: "QN".to_string(),
                room_description: "Queen sized bed".to_string(),
                total_rate: 139.00,
                total_rate_inclusive: 153.09,
            },
        };
        collections.insert_one(plan, None).await?;
    }

    let count = collections.count_documents(doc! { "hotelId": "3" }, None).await?;
    if count == 0 {
        let plan = RatePlan {
            hotel_id: "3".to_string(),
            code: "RACK".to_string(),
            in_date: "2015-04-09".to_string(),
            out_date: "2015-04-10".to_string(),
            room_type: RoomType {
                bookable_rate: 109.00,
                code: "KNG".to_string(),
                room_description: "King sized bed".to_string(),
                total_rate: 109.00,
                total_rate_inclusive: 123.17,
            },
        };
        collections.insert_one(plan, None).await?;
    }

    for i in 7..=80 {
        if i % 3 == 0 {
            let hotel_id = i.to_string();
            let count = collections.count_documents(doc! { "hotelId": hotel_id.as_str() }, None).await?;
            let mut end_date = "2015-04-".to_string();
            let mut rate = 109.00;
            let mut rate_inc = 123.17;
            if i % 2 == 0 {
                end_date += "17"
            } else {
                end_date += "24"
            }
            if i % 5 == 0 {
                rate = 120.00;
                rate_inc = 140.00;
            } else if i % 5 == 2 {
                rate = 124.00;
                rate_inc = 144.00;
            } else if i % 5 == 3 {
                rate = 132.00;
                rate_inc = 158.00;
            } else if i % 5 == 4 {
                rate = 232.00;
                rate_inc = 258.00;
            }

            if count == 0 {
                let plan = RatePlan {
                    hotel_id,
                    code: "RACK".to_string(),
                    in_date: "2015-04-09".to_string(),
                    out_date: end_date,
                    room_type: RoomType {
                        bookable_rate: rate,
                        code: "KNG".to_string(),
                        room_description: "King sized bed".to_string(),
                        total_rate: rate,
                        total_rate_inclusive: rate_inc,
                    },
                };
                collections.insert_one(plan, None).await?;
            }
        }
    }

    let index = IndexModel::builder().keys(doc! { "hotelId": 1 }).build();
    collections.create_index(index, None).await?;

    Ok(db)
}
