#!/usr/bin/env bash

pkill -9 launcher
pkill -9 koala
ssh danyang-06 "pkill -9 launcher; pkill -9 koala"
