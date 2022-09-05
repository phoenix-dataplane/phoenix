WORKDIR=`dirname $(realpath $0)`
cd $WORKDIR
docker-compose -f docker-compose-profile.yml -H "ssh://root@danyang-05" up -d
docker-compose -f docker-compose-geo.yml -H "ssh://root@danyang-06" up -d
docker-compose -f docker-compose-rate.yml -H "ssh://root@danyang-06" up -d
