use mongodb::bson::doc;
use mongodb::error::Result;
use mongodb::{Client, Database};
use mongodb::IndexModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    #[serde(rename = "hotelId")]
    pub id: String,
    pub lat: f64,
    pub lon: f64,
}

pub async fn initialize_database(uri: impl AsRef<str>) -> Result<Database> {
    let client = Client::with_uri_str(uri).await?;

    let db = client.database("geo-db");
    let collections = db.collection::<Point>("geo");

    let count = collections.count_documents(doc! { "hotelId": "1" }, None).await?;
    if count == 0 {
        let hotel = Point {
            id: "1".to_string(),
            lat: 37.7867,
            lon: -122.4112,
        };
        collections.insert_one(hotel, None).await?;
    }

    let count = collections.count_documents(doc! { "hotelId": "2" }, None).await?;
    if count == 0 {
        let hotel = Point {
            id: "2".to_string(),
            lat: 37.7854,
            lon: -122.4005,
        };
        collections.insert_one(hotel, None).await?;
    }

    let count = collections.count_documents(doc! { "hotelId": "3" }, None).await?;
    if count == 0 {
        let hotel = Point {
            id: "3".to_string(),
            lat: 37.7854,
            lon: -122.4071,
        };
        collections.insert_one(hotel, None).await?;
    }

    let count = collections.count_documents(doc! { "hotelId": "4" }, None).await?;
    if count == 0 {
        let hotel = Point {
            id: "4".to_string(),
            lat: 37.7936,
            lon: -122.3930,
        };
        collections.insert_one(hotel, None).await?;
    }

    let count = collections.count_documents(doc! { "hotelId": "5" }, None).await?;
    if count == 0 {
        let hotel = Point {
            id: "5".to_string(),
            lat: 37.7831,
            lon: -122.4181,
        };
        collections.insert_one(hotel, None).await?;
    }

    let count = collections.count_documents(doc! { "hotelId": "6" }, None).await?;
    if count == 0 {
        let hotel = Point {
            id: "6".to_string(),
            lat: 37.7863,
            lon: -122.4015,
        };
        collections.insert_one(hotel, None).await?;
    }

    for i in 7..=80 {
        let hotel_id = i.to_string();
        let count = collections.count_documents(doc! { "hotelId": &*hotel_id }, None).await?;
        let lat = 37.7835 + i as f64 / 500.0 * 3.0;
        let lon = -122.41 + i as f64 / 500.0 * 4.0;
        let hotel = Point {
            id: hotel_id,
            lat,
            lon,
        };
        if count == 0 {
            collections.insert_one(hotel, None).await?;
        }
    }

    let index = IndexModel::builder().keys(doc! { "hotelId": 1 }).build();
    collections.create_index(index, None).await?;

    Ok(db)
}
