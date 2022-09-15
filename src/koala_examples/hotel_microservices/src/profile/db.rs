use mongodb::bson::doc;
use mongodb::error::Result;
use mongodb::{Client, Database};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hotel {
    pub id: String,
    pub name: String,
    #[serde(rename = "phoneNumber")]
    pub phone_number: String,
    pub description: String,
    pub address: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    #[serde(rename = "streetNumber")]
    pub street_number: String,
    #[serde(rename = "streetName")]
    pub street_name: String,
    pub city: String,
    pub state: String,
    pub country: String,
    #[serde(rename = "postalCode")]
    pub postal_code: String,
    pub lat: f32,
    pub lon: f32,
}

pub async fn initialize_database(uri: impl AsRef<str>) -> Result<Database> {
    let client = Client::with_uri_str(uri).await?;
    log::info!("New session successful...");

    log::info!("Generating test data...");
    let db = client.database("profile-db");
    let collections = db.collection::<Hotel>("hotels");

    let count = collections.count_documents(doc! { "id": 1 }, None).await?;
    if count == 0 {
        let hotel = Hotel {
            id: "1".to_string(),
            name: "Clift Hotel".to_string(),
            phone_number: "(415) 775-4700".to_string(),
            description: String::from("A 6-minute walk from Union Square and 4 minutes from a Muni Metro station, this luxury hotel designed by Philippe Starck features an artsy furniture collection in the lobby, including work by Salvador Dali."), 
            address: Address {
                street_number: "495".to_string(),
                street_name: "Geary St".to_string(),
                city: "San Francisco".to_string(),
                state: "CA".to_string(),
                country: "United States".to_string(),
                postal_code: "94102".to_string(),
                lat: 37.7867,
                lon: -122.4112,
            },
        };
        collections.insert_one(hotel, None).await?;
    }
    let count = collections.count_documents(doc! { "id": 2 }, None).await?;
    if count == 0 {
        let hotel = Hotel {
            id: "2".to_string(),
            name: "W San Francisco".to_string(),
            phone_number: "(415) 777-5300".to_string(),
            description: String::from("Less than a block from the Yerba Buena Center for the Arts, this trendy hotel is a 12-minute walk from Union Square."), 
            address: Address {
                street_number: "181".to_string(),
                street_name: "3rd St".to_string(),
                city: "San Francisco".to_string(),
                state: "CA".to_string(),
                country: "United States".to_string(),
                postal_code: "94103".to_string(),
                lat: 37.7854,
                lon: -122.4005,
            },
        };
        collections.insert_one(hotel, None).await?;
    }
    let count = collections.count_documents(doc! { "id": 3 }, None).await?;
    if count == 0 {
        let hotel = Hotel {
            id: "3".to_string(),
            name: "Hotel Zetta".to_string(),
            phone_number: "(415) 543-8555".to_string(),
            description: String::from("A 3-minute walk from the Powell Street cable-car turnaround and BART rail station, this hip hotel 9 minutes from Union Square combines high-tech lodging with artsy touches."), 
            address: Address {
                street_number: "55".to_string(),
                street_name: "5th St".to_string(),
                city: "San Francisco".to_string(),
                state: "CA".to_string(),
                country: "United States".to_string(),
                postal_code: "94103".to_string(),
                lat: 37.7834,
                lon: -122.4071,
            },
        };
        collections.insert_one(hotel, None).await?;
    }
    let count = collections.count_documents(doc! { "id": 4 }, None).await?;
    if count == 0 {
        let hotel = Hotel {
            id: "4".to_string(),
            name: "Hotel Vitale".to_string(),
            phone_number: "(415) 278-3700".to_string(),
            description: String::from("This waterfront hotel with Bay Bridge views is 3 blocks from the Financial District and a 4-minute walk from the Ferry Building."), 
            address: Address {
                street_number: "8".to_string(),
                street_name: "Mission St".to_string(),
                city: "San Francisco".to_string(),
                state: "CA".to_string(),
                country: "United States".to_string(),
                postal_code: "94105".to_string(),
                lat: 37.7936,
                lon: -122.3930,
            },
        };
        collections.insert_one(hotel, None).await?;
    }
    let count = collections.count_documents(doc! { "id": 5 }, None).await?;
    if count == 0 {
        let hotel = Hotel {
            id: "5".to_string(),
            name: "Phoenix Hotel".to_string(),
            phone_number: "(415) 776-1380".to_string(),
            description: String::from("Located in the Tenderloin neighborhood, a 10-minute walk from a BART rail station, this retro motor lodge has hosted many rock musicians and other celebrities since the 1950s. It’s a 4-minute walk from the historic Great American Music Hall nightclub."), 
            address: Address {
                street_number: "601".to_string(),
                street_name: "Eddy St".to_string(),
                city: "San Francisco".to_string(),
                state: "CA".to_string(),
                country: "United States".to_string(),
                postal_code: "94109".to_string(),
                lat: 37.7831,
                lon: -122.4181,
            },
        };
        collections.insert_one(hotel, None).await?;
    }
    let count = collections.count_documents(doc! { "id": 6 }, None).await?;
    if count == 0 {
        let hotel = Hotel {
            id: "6".to_string(),
            name: "St. Regis San Francisco".to_string(),
            phone_number: "(415) 284-4000".to_string(),
            description: String::from("St. Regis Museum Tower is a 42-story, 484 ft skyscraper in the South of Market district of San Francisco, California, adjacent to Yerba Buena Gardens, Moscone Center, PacBell Building and the San Francisco Museum of Modern Art."), 
            address: Address {
                street_number: "125".to_string(),
                street_name: "3rd St".to_string(),
                city: "San Francisco".to_string(),
                state: "CA".to_string(),
                country: "United States".to_string(),
                postal_code: "94109".to_string(),
                lat: 37.7863,
                lon: -122.4015,
            },
        };
        collections.insert_one(hotel, None).await?;
    }

    for i in 7..=80 {
        let hotel_id = i.to_string();
        let count = collections.count_documents(doc! { "id": hotel_id.as_str() }, None).await?;
        let phone_number = "(415) 284-40".to_string() + &hotel_id;
        let lat = 37.7835 + i as f32 / 500.0 * 3.0;
        let lon = -122.41 + i as f32 / 500.0 * 4.0;
        if count == 0 {
            let hotel = Hotel {
                id: hotel_id,
                name: "St. Regis San Francisco".to_string(),
                phone_number,
                description: String::from("St. Regis Museum Tower is a 42-story, 484 ft skyscraper in the South of Market district of San Francisco, California, adjacent to Yerba Buena Gardens, Moscone Center, PacBell Building and the San Francisco Museum of Modern Art."), 
                address: Address {
                    street_number: "125".to_string(),
                    street_name: "3rd St".to_string(),
                    city: "San Francisco".to_string(),
                    state: "CA".to_string(),
                    country: "United States".to_string(),
                    postal_code: "94109".to_string(),
                    lat,
                    lon,
                },
            };
            collections.insert_one(hotel, None).await?;
        }
    }

    Ok(db)
}
